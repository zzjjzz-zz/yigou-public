use crate::{
    backups,
    devices::{AppState, NewDevice},
    security::Credential,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tauri::State;
type Reply = Result<Value, String>;
fn reply(result: anyhow::Result<Value>) -> Reply {
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn device_add(
    state: State<'_, Arc<AppState>>,
    device: NewDevice,
    credential: Credential,
) -> Reply {
    reply(state.add(device, credential).await)
}
#[tauri::command]
pub async fn device_list(state: State<'_, Arc<AppState>>) -> Reply {
    reply(state.list().await.map(|v| json!(v)))
}
#[tauri::command]
pub async fn device_connect(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    reply(state.connect(&device_id).await)
}
#[tauri::command]
pub async fn device_disconnect(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    reply(
        state
            .disconnect(&device_id)
            .await
            .map(|_| json!({"disconnected":true})),
    )
}
#[tauri::command]
pub async fn device_info(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    reply(state.query(&device_id, "version").await)
}
#[tauri::command]
pub async fn device_query(
    state: State<'_, Arc<AppState>>,
    device_id: String,
    query: String,
) -> Reply {
    reply(state.query(&device_id, &query).await)
}
#[tauri::command]
pub async fn device_discover(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    reply(state.query(&device_id, "neighbors").await)
}
#[tauri::command]
pub async fn device_backup(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    reply(state.backup(&device_id).await)
}
#[tauri::command]
pub async fn config_execute_approved(
    state: State<'_, Arc<AppState>>,
    device_id: String,
    plan_id: String,
    reviewed_hash: String,
    approval: crate::devices::configure::ConfigApproval,
) -> Reply {
    reply(
        state
            .execute_config_plan(&device_id, &plan_id, &reviewed_hash, approval)
            .await,
    )
}
#[tauri::command]
pub async fn host_key_probe(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    reply(state.probe(&device_id).await)
}
#[tauri::command]
pub async fn host_key_trust(
    state: State<'_, Arc<AppState>>,
    device_id: String,
    algorithm: String,
    key: String,
    confirmed_fingerprint: String,
) -> Reply {
    reply(
        state
            .trust(&device_id, &algorithm, &key, &confirmed_fingerprint)
            .await
            .map(|_| json!({"trusted":true})),
    )
}
#[tauri::command]
pub async fn terminal_open(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    reply(state.terminal_open(&device_id).await)
}
#[tauri::command]
pub async fn terminal_exec(
    state: State<'_, Arc<AppState>>,
    session: String,
    command: String,
) -> Reply {
    reply(state.terminal_exec(&session, &command).await)
}
#[tauri::command]
pub async fn terminal_close(state: State<'_, Arc<AppState>>, session: String) -> Reply {
    reply(
        state
            .terminal_close(&session)
            .await
            .map(|_| json!({"closed":true})),
    )
}
#[tauri::command]
pub async fn terminal_read(state: State<'_, Arc<AppState>>, session: String) -> Reply {
    reply(state.terminal_read(&session).await)
}
#[tauri::command]
pub async fn terminal_write(
    state: State<'_, Arc<AppState>>,
    session: String,
    data: String,
) -> Reply {
    let data = zeroize::Zeroizing::new(data);
    reply(state.terminal_write(&session, &data).await)
}
#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, Arc<AppState>>,
    session: String,
    cols: u16,
    rows: u16,
) -> Reply {
    reply(state.terminal_resize(&session, cols, rows).await)
}
#[tauri::command]
pub fn backup_history(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    reply(state.store.backups(&device_id).map(|v| json!(v)))
}
#[tauri::command]
pub fn backup_verify(
    state: State<'_, Arc<AppState>>,
    device_id: String,
    backup_id: String,
) -> Reply {
    reply((|| {
        let records = state.store.backups(&device_id)?;
        let record = records
            .iter()
            .find(|r| r["id"] == backup_id)
            .ok_or_else(|| anyhow::anyhow!("BACKUP_NOT_FOUND"))?;
        backups::verify(&state.root.join("backups"), state.vault.as_ref(), record)
    })())
}
#[tauri::command]
pub fn task_status(state: State<'_, Arc<AppState>>, task_id: Option<String>) -> Reply {
    reply(state.store.tasks(task_id.as_deref()).map(|v| json!(v)))
}
#[tauri::command]
pub async fn backup_reindex(state: State<'_, Arc<AppState>>, device_id: String) -> Reply {
    let _lock = state.backup_lock.lock().await;
    reply((|| {
        let task = state.store.start(Some(&device_id), "backup_reindex")?;
        let guard = crate::tasks::TaskGuard::new(&state.store, &task);
        let result = backups::reindex(
            &state.root.join("backups"),
            &state.store,
            state.vault.as_ref(),
            &device_id,
        );
        let error = result.as_ref().err().map(|e| e.to_string());
        guard.finish(error.as_deref())?;
        result
    })())
}
#[tauri::command]
pub fn model_config_get(state: State<'_, Arc<AppState>>) -> Reply {
    reply(
        state
            .store
            .setting("model")
            .map(|v| v.unwrap_or(Value::Null)),
    )
}
#[tauri::command]
pub fn model_config_save(
    state: State<'_, Arc<AppState>>,
    base_url: String,
    model: String,
    api_key: String,
) -> Reply {
    let key = zeroize::Zeroizing::new(api_key);
    reply(state.model_save(&base_url, &model, &key))
}
#[tauri::command]
pub async fn ai_chat(
    state: State<'_, Arc<AppState>>,
    device_id: String,
    prompt: String,
    request_id: String,
) -> Reply {
    reply(
        state
            .inner()
            .chat(&device_id, &prompt, &request_id, false)
            .await,
    )
}
#[tauri::command]
pub async fn ai_plan(
    state: State<'_, Arc<AppState>>,
    device_id: String,
    prompt: String,
    request_id: String,
) -> Reply {
    reply(
        state
            .inner()
            .chat(&device_id, &prompt, &request_id, true)
            .await,
    )
}
#[tauri::command]
pub async fn ai_autonomous(
    state: State<'_, Arc<AppState>>,
    device_id: String,
    prompt: String,
    request_id: String,
    consent: bool,
) -> Reply {
    reply(
        state
            .inner()
            .autonomous_chat(&device_id, &prompt, &request_id, consent)
            .await,
    )
}
#[tauri::command]
pub async fn system_status(state: State<'_, Arc<AppState>>) -> Reply {
    let worker = state
        .rpc(json!({"op":"ping"}))
        .await
        .map_err(|e| e.to_string())?;
    if std::env::var_os("YIGOU_SMOKE").is_some() {
        eprintln!("YIGOU_IPC_READY: frontend -> Rust -> Python protocol=1");
    }
    Ok(
        json!({"worker":worker,"data_dir":state.root,"ai_mode":"optional_autonomous","terminal_mode":"interactive","version":env!("CARGO_PKG_VERSION")}),
    )
}
