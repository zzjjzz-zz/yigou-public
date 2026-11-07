pub mod configure;
mod terminal;
pub mod worker;
use crate::{
    backups,
    security::{Credential, Vault, fingerprint, private_dir, private_write},
    tasks::Store,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use terminal::TerminalSession;
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;
use worker::WorkerClient;
use zeroize::Zeroizing;

pub type EventSink = Arc<dyn Fn(Value) + Send + Sync>;

/// Device platforms and their capability tiers (must mirror worker query.py
/// DRIVERS flags exactly — both layers enforce independently).
/// Tier 1: enroll / host-key trust / SSH session / manual PTY.
/// Tier 2: + whitelisted read-only queries (per-platform subset).
/// Tier 3: + backup and AI configure (original three vendors only, pending
/// the same scripted save/read-back certification for any newcomer).
pub const VENDORS: &[(&str, bool, bool)] = &[
    // (vendor, allow_backup, allow_configure)
    ("huawei", true, true),
    ("ruijie", true, true),
    ("h3c", true, true),
    ("cisco_ios", false, false),
    ("cisco_xe", false, false),
    ("maipu", false, false),
    ("raisecom_ros", false, false),
    ("raisecom_roap", false, false),
    ("huawei_olt", false, false),
    ("huawei_smartax", false, false),
    ("zte_zxros", false, false),
];

pub fn vendor_exists(vendor: &str) -> bool {
    VENDORS.iter().any(|(v, _, _)| *v == vendor)
}

pub fn vendor_allows_backup(vendor: &str) -> bool {
    VENDORS
        .iter()
        .find(|(v, _, _)| *v == vendor)
        .is_some_and(|(_, b, _)| *b)
}

pub fn vendor_allows_configure(vendor: &str) -> bool {
    VENDORS
        .iter()
        .find(|(v, _, _)| *v == vendor)
        .is_some_and(|(_, _, c)| *c)
}

/// Read-only queries a platform offers in the UI (mirrors worker whitelist).
pub fn vendor_queries(vendor: &str) -> &'static [&'static str] {
    match vendor {
        "huawei" | "ruijie" | "h3c" => &["version", "ports", "config", "neighbors", "startup"],
        "cisco_ios" | "cisco_xe" => &["version", "ports", "config", "neighbors", "startup"],
        "maipu" | "raisecom_ros" => &["version", "config", "startup"],
        "huawei_olt" | "huawei_smartax" => &["version", "config"],
        "raisecom_roap" | "zte_zxros" => &["version"],
        _ => &[],
    }
}

pub struct AppState {
    pub root: PathBuf,
    pub store: Store,
    pub vault: Arc<dyn Vault>,
    pub worker: Mutex<WorkerClient>,
    pub sessions: Mutex<HashMap<String, String>>,
    pub terminals: Mutex<HashMap<String, Arc<TerminalSession>>>,
    terminal_launch: (PathBuf, Vec<String>),
    terminal_slots: Arc<Semaphore>,
    pub backup_lock: Mutex<()>,
    pub config_lock: Mutex<()>,
    pub ai_lock: Mutex<()>,
    pub conversations: Mutex<HashMap<String, Vec<rig_core::message::Message>>>,
    pub events: EventSink,
}
#[derive(Serialize, Deserialize)]
pub struct NewDevice {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub vendor: String,
    pub username: String,
    pub auto_backup: bool,
    pub backup_minutes: u32,
}
impl NewDevice {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || self.name.chars().any(char::is_control)
            || self.username.trim().is_empty()
            || self.username.len() > 128
            || self.username.chars().any(char::is_control)
        {
            bail!("DEVICE_INVALID: 设备名称或用户名无效");
        }
        let host_ok = self.host.parse::<std::net::IpAddr>().is_ok()
            || (!self.host.is_empty()
                && self.host.len() < 254
                && self
                    .host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                && !self.host.starts_with('-'));
        if !host_ok
            || self.port == 0
            || !vendor_exists(&self.vendor)
            || !(5..=10080).contains(&self.backup_minutes)
        {
            bail!("DEVICE_INVALID: 地址、厂商、端口或备份周期无效");
        }
        if self.auto_backup && !vendor_allows_backup(&self.vendor) {
            bail!("DEVICE_INVALID: 该厂商暂不支持自动备份（只读阶段），请关闭自动备份选项");
        }
        Ok(())
    }
}
impl AppState {
    pub fn new(
        root: PathBuf,
        worker: WorkerClient,
        vault: Arc<dyn Vault>,
        events: EventSink,
    ) -> Result<Arc<Self>> {
        private_dir(&root)?;
        private_dir(&root.join("backups"))?;
        private_dir(&root.join("hosts"))?;
        let terminal_launch = worker.launch_spec();
        Ok(Arc::new(Self {
            store: Store::open(&root.join("yigou.sqlite3"))?,
            root,
            vault,
            worker: Mutex::new(worker),
            sessions: Mutex::new(HashMap::new()),
            terminals: Mutex::new(HashMap::new()),
            terminal_launch,
            terminal_slots: Arc::new(Semaphore::new(16)),
            backup_lock: Mutex::new(()),
            config_lock: Mutex::new(()),
            ai_lock: Mutex::new(()),
            conversations: Mutex::new(HashMap::new()),
            events,
        }))
    }
    pub fn emit(&self, value: Value) {
        (self.events)(value);
    }
    pub async fn add(&self, device: NewDevice, credential: Credential) -> Result<Value> {
        device.validate()?;
        if !matches!(credential.kind.as_str(), "password" | "key")
            || (credential.kind == "password" && credential.secret.is_empty())
            || (credential.kind == "key" && credential.key_path.as_deref().unwrap_or("").is_empty())
        {
            bail!("CREDENTIAL_INVALID: 请提供密码或私钥文件路径");
        }
        let id = Uuid::new_v4().to_string();
        let mut value = serde_json::to_value(device)?;
        value["id"] = json!(id);
        let secret = Zeroizing::new(serde_json::to_string(&credential)?);
        self.vault.set(&id, &secret)?;
        if let Err(error) = self.store.save_device(&value) {
            let _ = self.vault.delete(&id);
            return Err(error);
        }
        Ok(value)
    }
    pub async fn list(&self) -> Result<Vec<Value>> {
        let mut devices = self.store.devices()?;
        let sessions = self.sessions.lock().await;
        for d in &mut devices {
            let id = d["id"].as_str().context("DEVICE_ID")?.to_string();
            d["connected"] = json!(sessions.contains_key(&id));
            d["host_key_trusted"] = json!(self.root.join("hosts").join(id).is_file());
        }
        Ok(devices)
    }
    pub async fn rpc(&self, request: Value) -> Result<Value> {
        let mut worker = self.worker.lock().await;
        if !worker.is_started() && !self.sessions.lock().await.is_empty() {
            self.sessions.lock().await.clear();
            self.emit(json!({"kind":"sessions_reset"}));
        }
        let result = worker.call(request).await;
        drop(worker);
        if let Err(ref err) = result
            && err.to_string().starts_with("WORKER_")
        {
            self.sessions.lock().await.clear();
            self.emit(json!({"kind":"sessions_reset"}));
        }
        result
    }
    pub async fn tracked(
        &self,
        device: Option<&str>,
        operation: &str,
        request: Value,
    ) -> Result<Value> {
        let task = self.store.start(device, operation)?;
        let guard = crate::tasks::TaskGuard::new(&self.store, &task);
        self.emit(json!({"kind":"task","id":task,"status":"running","device_id":device,"operation":operation}));
        let result = self.rpc(request).await;
        let error = result.as_ref().err().map(|e| e.to_string());
        guard.finish(error.as_deref())?;
        self.emit(json!({"kind":"task","id":task,"status":if error.is_some(){"failed"}else{"succeeded"},"error":error,"device_id":device,"operation":operation}));
        result
    }
    pub async fn probe(&self, id: &str) -> Result<Value> {
        let device = self.store.device(id)?;
        self.tracked(
            Some(id),
            "host_key_probe",
            json!({"op":"probe","device":device}),
        )
        .await
    }
    pub async fn trust(&self, id: &str, algorithm: &str, key: &str, confirmed: &str) -> Result<()> {
        let device = self.store.device(id)?;
        if fingerprint(key)? != confirmed
            || !matches!(
                algorithm,
                "ssh-ed25519"
                    | "ssh-rsa"
                    | "ecdsa-sha2-nistp256"
                    | "ecdsa-sha2-nistp384"
                    | "ecdsa-sha2-nistp521"
            )
        {
            bail!("HOST_KEY_MISMATCH: 手工核实的指纹与密钥不匹配");
        }
        let host = device["host"].as_str().context("DEVICE_HOST")?;
        let port = device["port"].as_u64().context("DEVICE_PORT")?;
        let hostname = if port == 22 {
            host.to_string()
        } else {
            format!("[{host}]:{port}")
        };
        // Serialize the whole trust-file replacement with connect, disconnect,
        // plan creation and device configuration. Do not reenter config_lock.
        let _config = self.config_lock.lock().await;
        let path = self.root.join("hosts").join(id);
        let temp = path.with_extension(Uuid::new_v4().to_string());
        private_write(&temp, format!("{hostname} {algorithm} {key}\n").as_bytes())?;
        if let Err(error) = self.disconnect_locked(id).await {
            let _ = std::fs::remove_file(&temp);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        let task = self.store.start(Some(id), "host_key_trust")?;
        self.store.finish(&task, None)?;
        Ok(())
    }
    async fn open_session(&self, id: &str) -> Result<Value> {
        let device = self.store.device(id)?;
        let secret = Zeroizing::new(
            self.vault
                .get(id)?
                .context("CREDENTIAL_UNAVAILABLE: 系统凭据库中没有此设备凭据")?,
        );
        let credential: Credential = serde_json::from_str(&secret)?;
        let operation = "connect";
        self.tracked(Some(id),operation,json!({"op":operation,"device":device,"credential":credential,"known_hosts":self.root.join("hosts").join(id)})).await
    }
    pub async fn connect(&self, id: &str) -> Result<Value> {
        let _config = self.config_lock.lock().await;
        self.disconnect_query(id).await?;
        let mut result = self.open_session(id).await?;
        self.sessions.lock().await.insert(
            id.into(),
            result["session"].as_str().context("WORKER_SESSION")?.into(),
        );
        if self.store.device(id)?["auto_backup"] == true {
            result["backup"] = match self.backup(id).await {
                Ok(v) => json!({"status":"succeeded","record":v}),
                Err(e) => json!({"status":"failed","error":e.to_string()}),
            };
        }
        Ok(result)
    }
    async fn disconnect_query(&self, id: &str) -> Result<()> {
        let session = self.sessions.lock().await.remove(id);
        if let Some(session) = session {
            let _ = self.rpc(json!({"op":"close","session":session})).await;
        }
        self.conversations
            .lock()
            .await
            .retain(|key, _| !key.starts_with(&format!("{id}:")));
        Ok(())
    }
    pub async fn disconnect(&self, id: &str) -> Result<()> {
        let _config = self.config_lock.lock().await;
        self.disconnect_locked(id).await
    }
    async fn disconnect_locked(&self, id: &str) -> Result<()> {
        self.disconnect_query(id).await?;
        let terminal_ids: Vec<String> = self
            .terminals
            .lock()
            .await
            .iter()
            .filter(|(_, terminal)| terminal.device_id == id)
            .map(|(s, _)| s.clone())
            .collect();
        for session in terminal_ids {
            // disconnect_locked already owns config_lock; avoid reacquiring it.
            self.terminal_close_locked(&session).await?;
        }
        Ok(())
    }
    pub async fn session(&self, id: &str) -> Result<String> {
        self.sessions
            .lock()
            .await
            .get(id)
            .cloned()
            .context("NOT_CONNECTED: 设备未连接，请先核实主机密钥并连接设备")
    }
    pub async fn query(&self, id: &str, kind: &str) -> Result<Value> {
        self.query_internal(id, kind, false).await
    }

    // Only the host-side planning tool requests this private raw digest.
    // It must be removed before any model/UI/event serialization.
    pub(crate) async fn query_with_private_evidence(&self, id: &str) -> Result<Value> {
        self.query_internal(id, "config", true).await
    }

    async fn query_internal(&self, id: &str, kind: &str, private_evidence: bool) -> Result<Value> {
        if !matches!(
            kind,
            "version" | "ports" | "config" | "neighbors" | "startup"
        ) {
            bail!("QUERY_DENIED: 未开放的查询");
        }
        let device = self.store.device(id)?;
        let vendor = device["vendor"]
            .as_str()
            .context("DEVICE_VENDOR")?
            .to_string();
        if !vendor_queries(&vendor).contains(&kind) {
            bail!("QUERY_DENIED: 该厂商暂不支持此查询，命令证据不足");
        }
        if private_evidence && (kind != "config" || !vendor_allows_backup(&vendor)) {
            bail!("QUERY_DENIED: 私有原始配置证据仅限已经验证的可配置厂商");
        }
        let session = self.session(id).await?;
        let result = self
            .tracked(
                Some(id),
                &format!("query:{kind}"),
                json!({"op":"query","session":session,"query":kind,"include_raw_sha256":private_evidence}),
            )
            .await;
        if let Err(ref e) = result
            && (e.to_string().starts_with("QUERY_FAILED")
                || e.to_string().starts_with("NOT_CONNECTED"))
        {
            self.sessions.lock().await.remove(id);
        }
        result
    }
    pub async fn backup(&self, id: &str) -> Result<Value> {
        // Capability gate before any task tracking or device traffic.
        let backup_device = self.store.device(id)?;
        let backup_vendor = backup_device["vendor"]
            .as_str()
            .context("DEVICE_VENDOR")?
            .to_string();
        if !vendor_allows_backup(&backup_vendor) {
            bail!("OPERATION_UNSUPPORTED: 该厂商暂不支持配置备份（只读阶段），未连接设备");
        }
        let _guard = self.backup_lock.lock().await;
        let task = self.store.start(Some(id), "backup")?;
        let guard = crate::tasks::TaskGuard::new(&self.store, &task);
        self.emit(
            json!({"kind":"task","id":task,"device_id":id,"operation":"backup","status":"running"}),
        );
        let result = async {
            let session = self.session(id).await?;
            let mut raw = self.rpc(json!({"op":"backup","session":session})).await?;
            let content = raw["content"]
                .take()
                .as_str()
                .context("BACKUP_NO_CONTENT")?
                .to_string();
            backups::save(
                &self.root.join("backups"),
                &self.store,
                self.vault.as_ref(),
                id,
                content,
            )
        }
        .await;
        let error = result.as_ref().err().map(|e: &anyhow::Error| e.to_string());
        guard.finish(error.as_deref())?;
        self.store.save_setting(
            &format!("backup_attempt:{id}"),
            &json!(chrono::Utc::now().timestamp()),
        )?;
        self.emit(json!({"kind":"task","id":task,"device_id":id,"operation":"backup","status":if error.is_some(){"failed"}else{"succeeded"},"error":error}));
        result
    }
    pub async fn scheduler(self: Arc<Self>) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Ok(devices) = self.store.devices() {
                for d in devices {
                    let Some(id) = d["id"].as_str() else { continue };
                    if d["auto_backup"] != true || self.session(id).await.is_err() {
                        continue;
                    }
                    let last = self
                        .store
                        .setting(&format!("backup_attempt:{id}"))
                        .ok()
                        .flatten()
                        .and_then(|x| x.as_i64())
                        .unwrap_or(0);
                    if chrono::Utc::now().timestamp() - last
                        >= d["backup_minutes"].as_i64().unwrap_or(60) * 60
                    {
                        let _ = self.backup(id).await;
                    }
                }
            }
        }
    }
}
