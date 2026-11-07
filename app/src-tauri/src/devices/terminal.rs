use super::{AppState, WorkerClient};
use crate::{security::Credential, tasks::TaskGuard};
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit};
use zeroize::Zeroizing;

pub struct TerminalSession {
    pub(super) device_id: String,
    worker: Mutex<WorkerClient>,
    _slot: OwnedSemaphorePermit,
}

impl AppState {
    pub async fn terminal_open(&self, id: &str) -> Result<Value> {
        // Never open a terminal while a trusted host key is being replaced.
        let _config = self.config_lock.lock().await;
        let slot = self
            .terminal_slots
            .clone()
            .try_acquire_owned()
            .context("SESSION_LIMIT: 最多同时打开 16 个终端")?;
        let device = self.store.device(id)?;
        let secret = Zeroizing::new(
            self.vault
                .get(id)?
                .context("CREDENTIAL_UNAVAILABLE: 设备凭据不可用")?,
        );
        let credential: Credential = serde_json::from_str(&secret)?;
        let mut worker = WorkerClient::new(
            self.terminal_launch.0.clone(),
            self.terminal_launch.1.clone(),
        );
        let task = self.store.start(Some(id), "terminal_open")?;
        let guard = TaskGuard::new(&self.store, &task);
        let result=worker.call(json!({"op":"terminal_open","device":device,"credential":credential,"known_hosts":self.root.join("hosts").join(id)})).await;
        guard.finish(result.as_ref().err().map(|e| e.to_string()).as_deref())?;
        let result = result?;
        let session = result["session"]
            .as_str()
            .context("WORKER_SESSION")?
            .to_string();
        self.terminals.lock().await.insert(
            session,
            Arc::new(TerminalSession {
                device_id: id.into(),
                worker: Mutex::new(worker),
                _slot: slot,
            }),
        );
        Ok(result)
    }

    async fn terminal_rpc(&self, session: &str, mut request: Value) -> Result<Value> {
        let terminal = self
            .terminals
            .lock()
            .await
            .get(session)
            .cloned()
            .context("TERMINAL_CLOSED: 终端会话已结束")?;
        request["session"] = json!(session);
        let result = terminal.worker.lock().await.call(request).await;
        let error = result.as_ref().err().map(|e| e.to_string());
        let broken = error.as_deref().is_some_and(|e| {
            e.starts_with("WORKER_")
                || e.starts_with("TERMINAL_IO_FAILED")
                || e.starts_with("NOT_CONNECTED")
        });
        let closed = result.as_ref().is_ok_and(|value| value["closed"] == true);
        if (closed || broken) && self.terminals.lock().await.remove(session).is_some() {
            let task = self
                .store
                .start(Some(&terminal.device_id), "terminal_closed")?;
            self.store.finish(&task, error.as_deref())?;
            self.emit(json!({"kind":"terminal_closed","session":session,"device_id":terminal.device_id,"error":error}));
        }
        result
    }

    pub async fn terminal_read(&self, session: &str) -> Result<Value> {
        self.terminal_rpc(session, json!({"op":"terminal_read"}))
            .await
    }
    pub async fn terminal_write(&self, session: &str, data: &str) -> Result<Value> {
        // A manual terminal must not change configuration between the baseline
        // recheck and the guarded batch write. Reject instead of queuing bytes
        // that might be applied only after the guarded operation ends.
        let _config = self.config_lock.try_lock().map_err(|_| {
            anyhow::anyhow!("CONFIG_BUSY: 设备配置或连接状态正在变更，当前终端输入未发送")
        })?;
        // This is a human PTY, not an AI tool. Never log input (including password prompts).
        self.terminal_rpc(session, json!({"op":"terminal_write","data":data}))
            .await
    }
    pub async fn terminal_resize(&self, session: &str, cols: u16, rows: u16) -> Result<Value> {
        self.terminal_rpc(
            session,
            json!({"op":"terminal_resize","cols":cols,"rows":rows}),
        )
        .await
    }
    #[cfg(test)]
    pub(crate) async fn hold_terminal_worker_for_test(
        &self,
        session: &str,
        entered: tokio::sync::oneshot::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        let terminal = self
            .terminals
            .lock()
            .await
            .get(session)
            .cloned()
            .context("TERMINAL_CLOSED")?;
        let _worker = terminal.worker.lock().await;
        let _ = entered.send(());
        let _ = release.await;
        Ok(())
    }

    pub async fn terminal_exec(&self, session: &str, command: &str) -> Result<Value> {
        let line = if command.ends_with(['\r', '\n']) {
            command.to_string()
        } else {
            format!("{command}\r")
        };
        self.terminal_write(session, &STANDARD.encode(line.as_bytes()))
            .await
    }
    pub async fn terminal_close(&self, session: &str) -> Result<()> {
        // Keep the terminal registered until the remote SSH close completes.
        // Config execution takes the same lock before checking terminal presence.
        let _config = self.config_lock.lock().await;
        self.terminal_close_locked(session).await
    }

    pub(super) async fn terminal_close_locked(&self, session: &str) -> Result<()> {
        let Some(terminal) = self.terminals.lock().await.get(session).cloned() else {
            return Ok(());
        };
        // Keep this session in the authoritative map throughout SSH teardown.
        // If the future is cancelled while another read owns worker.lock, the
        // still-live remote terminal must continue blocking automatic writes.
        let task = self
            .store
            .start(Some(&terminal.device_id), "terminal_close")?;
        let guard = TaskGuard::new(&self.store, &task);
        let result = terminal
            .worker
            .lock()
            .await
            .call(json!({"op":"close","session":session}))
            .await;
        guard.finish(result.as_ref().err().map(|e| e.to_string()).as_deref())?;
        result?;
        self.terminals.lock().await.remove(session);
        Ok(())
    }
}
