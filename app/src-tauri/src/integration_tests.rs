use crate::{
    devices::{AppState, NewDevice, worker::WorkerClient},
    security::{Credential, tests::MemoryVault},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

async fn terminal_output(state: &AppState, session: &str, expected: &str) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        loop {
            let result = state.terminal_read(session).await.unwrap();
            bytes.extend(STANDARD.decode(result["data"].as_str().unwrap()).unwrap());
            let output = String::from_utf8_lossy(&bytes).to_string();
            if output.contains(expected) {
                return output;
            }
            assert_ne!(result["closed"], true, "{output}");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn cancellation_kills_worker_without_poisoning_next_response() {
    let program = root().join("worker/.venv/bin/python");
    let code = "import sys,json,time\nfor line in sys.stdin:\n r=json.loads(line)\n if r['op']=='slow':time.sleep(5)\n print(json.dumps({'id':r['id'],'ok':True,'result':{'fresh':True}}),flush=True)";
    let mut worker = WorkerClient::new(program, vec!["-u".into(), "-c".into(), code.into()]);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            worker.call(serde_json::json!({"op":"slow"}))
        )
        .await
        .is_err()
    );
    assert!(!worker.is_started());
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        worker.call(serde_json::json!({"op":"ping"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(reply["fresh"], true);
}

#[tokio::test]
async fn rig_stream_calls_real_worker_over_local_ssh_and_never_receives_credentials() {
    let root = root();
    let python = root.join("worker/.venv/bin/python");
    let mut lab = Command::new(&python)
        .arg(root.join("tests/lab_server.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut output = BufReader::new(lab.stdout.take().unwrap());
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        output.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    let info: Value = serde_json::from_str(&line).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let vault = Arc::new(MemoryVault::default());
    let state = AppState::new(
        dir.path().to_path_buf(),
        WorkerClient::new(python, vec!["-m".into(), "yigou_worker".into()]),
        vault.clone(),
        Arc::new(move |event| recorded.lock().unwrap().push(event)),
    )
    .unwrap();
    let device = state
        .add(
            NewDevice {
                name: "LAB".into(),
                host: "127.0.0.1".into(),
                port: info["port"].as_u64().unwrap() as u16,
                vendor: "huawei".into(),
                username: "lab".into(),
                auto_backup: true,
                backup_minutes: 5,
            },
            Credential {
                kind: "password".into(),
                secret: "fixture-only".into(),
                key_path: None,
            },
        )
        .await
        .unwrap();
    let id = device["id"].as_str().unwrap();
    assert!(
        state
            .query(id, "ports")
            .await
            .unwrap_err()
            .to_string()
            .contains("NOT_CONNECTED")
    );
    assert!(state.connect(id).await.is_err());
    state
        .trust(
            id,
            info["algorithm"].as_str().unwrap(),
            info["key"].as_str().unwrap(),
            info["fingerprint"].as_str().unwrap(),
        )
        .await
        .unwrap();
    let connected = state.connect(id).await.unwrap();
    assert_eq!(connected["backup"]["status"], "succeeded");
    let base = info["model_base"].as_str().unwrap();
    state
        .model_save(&format!("{base}/v1"), "fixture-model", "fixture-key")
        .unwrap();
    let request = uuid::Uuid::new_v4().to_string();
    let reply = state
        .chat(id, "检查端口状态", &request, false)
        .await
        .unwrap();
    assert!(reply["text"].as_str().unwrap().contains("GE0/0/1"));
    assert_eq!(reply["tool_calls"], 1);
    assert_eq!(reply["executed"], false);
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "ai_delta")
    );
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "ai_tool_result" && e["result"]["ok"] == true)
    );
    let followup = state
        .chat(id, "继续分析", &uuid::Uuid::new_v4().to_string(), false)
        .await
        .unwrap();
    assert!(followup["text"].as_str().unwrap().contains("GE0/0/1"));
    let preview = state
        .chat(
            id,
            "生成端口描述修改计划",
            &uuid::Uuid::new_v4().to_string(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(preview["preview_only"], true);
    assert_eq!(preview["executed"], false);
    state
        .model_save(&format!("{base}/fail/v1"), "fixture-model", "fixture-key")
        .unwrap();
    assert!(
        state
            .chat(id, "检查端口", &uuid::Uuid::new_v4().to_string(), false)
            .await
            .unwrap_err()
            .to_string()
            .contains("MODEL_REQUEST_FAILED")
    );
    let terminal = state.terminal_open(id).await.unwrap();
    let session = terminal["session"].as_str().unwrap();
    terminal_output(&state, session, "<LAB>").await;
    assert_eq!(
        state.terminal_exec(session, "system-view").await.unwrap()["sent_bytes"],
        12
    );
    terminal_output(&state, session, "[LAB]").await;
    state
        .terminal_exec(
            session,
            "interface GE0/0/1\rdescription manual-marker\rreturn\rsave",
        )
        .await
        .unwrap();
    terminal_output(&state, session, "[Y/N]:").await;
    state
        .terminal_write(session, &STANDARD.encode(b"y\r"))
        .await
        .unwrap();
    terminal_output(&state, session, "Save complete").await;
    state.terminal_exec(session, "ask-secret").await.unwrap();
    terminal_output(&state, session, "Password:").await;
    state
        .terminal_exec(session, "manual-password")
        .await
        .unwrap();
    assert!(
        !terminal_output(&state, session, "Authenticated")
            .await
            .contains("manual-password")
    );
    // A busy query/AI worker cannot delay opening or using manual terminals.
    let query_lock = state.worker.lock().await;
    let independent =
        tokio::time::timeout(std::time::Duration::from_secs(5), state.terminal_open(id))
            .await
            .unwrap()
            .unwrap();
    let second = independent["session"].as_str().unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        state.terminal_exec(second, "system-view"),
    )
    .await
    .unwrap()
    .unwrap();
    terminal_output(&state, second, "[LAB]").await;
    drop(query_lock);
    state.terminal_close(session).await.unwrap();
    assert!(state.terminal_read(session).await.is_err());
    state
        .terminal_exec(second, "display version")
        .await
        .unwrap();
    terminal_output(&state, second, "Huawei").await;
    // Reconnecting queries preserves the user's config-mode terminal.
    state.connect(id).await.unwrap();
    state
        .terminal_exec(second, "display version")
        .await
        .unwrap();
    terminal_output(&state, second, "[LAB]").await;
    state
        .model_save(&format!("{base}/v1"), "fixture-model", "fixture-key")
        .unwrap();
    state
        .chat(id, "再次查询端口", &uuid::Uuid::new_v4().to_string(), false)
        .await
        .unwrap();
    // A failed query subprocess invalidates only query sessions.
    *state.worker.lock().await = WorkerClient::new(PathBuf::from("/bin/false"), vec![]);
    assert!(state.rpc(serde_json::json!({"op":"ping"})).await.is_err());
    assert!(state.sessions.lock().await.is_empty());
    state.terminal_exec(second, "return").await.unwrap();
    terminal_output(&state, second, "<LAB>").await;
    state.disconnect(id).await.unwrap();
    assert!(state.terminals.lock().await.is_empty());
    assert!(
        state
            .chat(id, "检查端口", &uuid::Uuid::new_v4().to_string(), false)
            .await
            .unwrap_err()
            .to_string()
            .contains("NOT_CONNECTED")
    );
    assert!(state.backup(id).await.is_err());
    assert_eq!(state.store.backups(id).unwrap().len(), 2);
    assert!(
        state
            .store
            .tasks(None)
            .unwrap()
            .iter()
            .all(|t| t["status"] != "running")
    );
    let event_text = serde_json::to_string(&*events.lock().unwrap()).unwrap();
    assert!(!event_text.contains("fixture-sensitive"));
    assert!(!event_text.contains("fixture-only"));
    assert!(!event_text.contains("fixture-key"));
    assert!(!event_text.contains("manual-password"));
    assert!(!event_text.contains("manual-marker"));
    let audit = serde_json::to_string(&state.store.tasks(None).unwrap()).unwrap();
    assert!(!audit.contains("manual-password"));
    assert!(!audit.contains("manual-marker"));
    let metadata = std::fs::read(dir.path().join("yigou.sqlite3")).unwrap();
    assert!(!String::from_utf8_lossy(&metadata).contains("fixture-only"));
    // Fetch only the local fixture's captured model requests and inspect the credential boundary.
    let raw=Command::new(root.join("worker/.venv/bin/python")).args(["-c","import urllib.request,sys;sys.stdout.buffer.write(urllib.request.urlopen(sys.argv[1]).read())",&format!("{base}/stats")]).output().await.unwrap();
    let captured = String::from_utf8(raw.stdout).unwrap();
    assert!(captured.contains("GE0/0/1"));
    assert!(!captured.contains("fixture-only"));
    assert!(!captured.contains("fixture-sensitive"));
    assert!(!captured.contains("manual-password"));
    assert!(!captured.contains("manual-marker"));
    assert!(!captured.contains("terminal_write"));
    drop(lab.stdin.take());
    lab.wait().await.unwrap();
}

#[tokio::test]
async fn closed_worker_is_restarted_and_sessions_are_invalidated() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(
        dir.path().to_path_buf(),
        WorkerClient::new(PathBuf::from("/bin/false"), vec![]),
        Arc::new(MemoryVault::default()),
        Arc::new(|_| {}),
    )
    .unwrap();
    state
        .sessions
        .lock()
        .await
        .insert("device".into(), "stale".into());
    assert!(state.rpc(serde_json::json!({"op":"ping"})).await.is_err());
    assert!(state.sessions.lock().await.is_empty());
}
