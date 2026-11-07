use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};
use uuid::Uuid;

pub struct WorkerClient {
    program: PathBuf,
    args: Vec<String>,
    process: Option<(Child, ChildStdin, BufReader<ChildStdout>)>,
}
impl WorkerClient {
    pub fn new(program: PathBuf, args: Vec<String>) -> Self {
        Self {
            program,
            args,
            process: None,
        }
    }
    async fn spawn(&mut self) -> Result<()> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        // The frozen worker is a console PE, but its only interface is private
        // JSONL pipes. Do not open Windows Terminal or steal desktop focus.
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = command
            .spawn()
            .context("WORKER_UNAVAILABLE: 无法启动设备执行程序")?;
        let input = child.stdin.take().context("WORKER_STDIN")?;
        let output = BufReader::new(child.stdout.take().context("WORKER_STDOUT")?);
        self.process = Some((child, input, output));
        Ok(())
    }
    pub async fn call(&mut self, mut request: Value) -> Result<Value> {
        if self.process.is_none() {
            self.spawn().await?;
        }
        let id = Uuid::new_v4().to_string();
        request["id"] = json!(id);
        let operation = request["op"].as_str().unwrap_or("").to_string();
        // Own the child in this future: cancelling a request kills it instead of
        // leaving an unread response to be mistaken for the next RPC response.
        let mut process = self.process.take().context("WORKER_UNAVAILABLE")?;
        let timeout = if operation == "configure" { 240 } else { 70 };
        let result = tokio::time::timeout(Duration::from_secs(timeout), async {
            let (_, input, output) = &mut process;
            let mut bytes = serde_json::to_vec(&request)?;
            bytes.push(b'\n');
            input.write_all(&bytes).await?;
            input.flush().await?;
            use zeroize::Zeroize;
            bytes.zeroize();
            drop(request);
            let mut line = Vec::new();
            // fill_buf avoids an unbounded read allocation from a malfunctioning child.
            loop {
                let available = output.fill_buf().await?;
                if available.is_empty() {
                    bail!("WORKER_EXITED: 设备执行程序已退出");
                }
                let end = available.iter().position(|b| *b == b'\n');
                let count = end.map(|p| p + 1).unwrap_or(available.len());
                if line.len() + count > 12 * 1024 * 1024 {
                    bail!("WORKER_FRAME_LIMIT");
                }
                line.extend_from_slice(&available[..count]);
                output.consume(count);
                if end.is_some() {
                    break;
                }
            }
            let response: Value = serde_json::from_slice(&line)?;
            line.zeroize();
            if response["id"] != id {
                bail!("WORKER_PROTOCOL: 响应编号不匹配");
            }
            Ok::<Value, anyhow::Error>(response)
        })
        .await;
        match result {
            Ok(Ok(response)) => {
                self.process = Some(process);
                if response["ok"] != true {
                    // Codes/messages are fixed strings produced by our worker; no raw SSH errors.
                    bail!(
                        "{}: {}",
                        response["error"]["code"].as_str().unwrap_or("WORKER_ERROR"),
                        response["error"]["message"].as_str().unwrap_or("执行失败")
                    );
                }
                Ok(response["result"].clone())
            }
            other => {
                let _ = process.0.kill().await;
                let _ = process.0.wait().await;
                match other {
                    Ok(Err(_)) => {
                        bail!(
                            "WORKER_FAILED: {operation} 执行程序异常，本执行程序中的 SSH 会话失效"
                        )
                    }
                    _ => bail!("WORKER_TIMEOUT: {operation} 超时，本执行程序中的 SSH 会话失效"),
                }
            }
        }
    }
    pub fn is_started(&self) -> bool {
        self.process.is_some()
    }
    pub fn launch_spec(&self) -> (PathBuf, Vec<String>) {
        (self.program.clone(), self.args.clone())
    }
}
