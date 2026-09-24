mod credentials;
mod engine;
mod model;
mod service;
mod setup;
#[cfg(test)]
mod tests;
use serde_json::Value;
use std::sync::{atomic::Ordering, Arc};
use tauri::{Emitter, Manager, State};

#[tauri::command]
fn snapshot(service: State<'_, Arc<service::Service>>) -> model::Store {
    service.snapshot()
}
#[tauri::command]
async fn request(
    operation: String,
    payload: Value,
    service: State<'_, Arc<service::Service>>,
) -> model::Result<Value> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.request(&operation, payload))
        .await
        .map_err(|_| "操作执行失败".to_string())?
}
#[tauri::command]
fn exit_app(app: tauri::AppHandle, service: State<'_, Arc<service::Service>>) -> model::Result<()> {
    if service.active() {
        return Err("仍有同步操作正在进行，请稍后退出，或最小化窗口继续运行".into());
    }
    service.stop.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            let service = service::Service::new(dir).map_err(std::io::Error::other)?;
            service.start();
            app.manage(service);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("close-requested", ());
            }
        })
        .invoke_handler(tauri::generate_handler![snapshot, request, exit_app])
        .run(tauri::generate_context!())
        .expect("Unable to launch Git Sync");
}
