#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager};
use yigou_core::{
    commands::*,
    devices::{AppState, worker::WorkerClient},
    security::SystemVault,
};

fn main() {
    let context = tauri::generate_context!();
    let result = tauri::Builder::default()
        .setup(|app| {
            let development = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            let root = if cfg!(debug_assertions) {
                std::env::var_os("YIGOU_DATA_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| development.join("data"))
            } else {
                app.path().app_data_dir()?
            };
            let worker = if cfg!(debug_assertions) {
                WorkerClient::new(
                    development.join(if cfg!(windows) {
                        "worker/.venv/Scripts/python.exe"
                    } else {
                        "worker/.venv/bin/python"
                    }),
                    vec!["-m".into(), "yigou_worker".into()],
                )
            } else {
                let exe = std::env::current_exe()?;
                WorkerClient::new(
                    exe.parent()
                        .ok_or("missing executable parent")?
                        .join(if cfg!(windows) {
                            "yigou-worker.exe"
                        } else {
                            "yigou-worker"
                        }),
                    vec![],
                )
            };
            let handle = app.handle().clone();
            let webview_data = root.join("webview");
            let state = AppState::new(
                root,
                worker,
                Arc::new(SystemVault),
                Arc::new(move |event| {
                    let _ = handle.emit("yigou:event", event);
                }),
            )?;
            app.manage(state.clone());
            tauri::async_runtime::spawn(state.scheduler());
            let mut window =
                tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?;
            if cfg!(debug_assertions) {
                window = window.data_directory(webview_data);
            }
            window.build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            device_add,
            device_list,
            device_connect,
            device_disconnect,
            device_info,
            device_query,
            device_discover,
            device_backup,
            config_execute_approved,
            host_key_probe,
            host_key_trust,
            terminal_open,
            terminal_exec,
            terminal_read,
            terminal_write,
            terminal_resize,
            terminal_close,
            backup_history,
            backup_verify,
            backup_reindex,
            task_status,
            model_config_get,
            model_config_save,
            ai_chat,
            ai_plan,
            ai_autonomous,
            system_status
        ])
        .run(context);
    if let Err(error) = result {
        eprintln!("意构客户端启动失败：{error}");
        std::process::exit(1);
    }
}
