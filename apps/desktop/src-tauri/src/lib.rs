pub mod backend;
pub mod bindings;
pub mod deep_link;
pub mod types;
mod watch;
use backend::Backend;
use std::sync::Arc;
use tauri::Manager;
use types::*;

macro_rules! handlers {
    ($(($name:ident,$arg:ty,$result:ty)),* $(,)?) => { $(
        #[tauri::command]
        async fn $name(state:tauri::State<'_,Arc<Backend>>,request:$arg)->Result<$result,String>{
            let backend=state.inner().clone();
            tauri::async_runtime::spawn_blocking(move ||backend.$name(request)).await.map_err(|e|e.to_string())?
        }
    )* };
}
bindings::command_table!(handlers);

pub fn run() {
    macro_rules! register {($(($name:ident,$arg:ty,$result:ty)),* $(,)?)=>{tauri::generate_handler![$($name),*]};}
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(bindings::command_table!(register))
        .setup(|app| {
            let data = ariad_host::packs::data_dir()?;
            let backend =
                Backend::new(&data, std::env::current_exe()?).map_err(std::io::Error::other)?;
            app.manage(backend);
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                app.deep_link().register_all()?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("desktop runtime failed");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            app.state::<Arc<Backend>>().shutdown();
        }
    });
}
