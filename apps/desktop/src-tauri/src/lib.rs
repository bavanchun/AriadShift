pub mod backend;
pub mod bindings;
mod command_table;
pub mod deep_link;
pub mod file_open;
#[cfg(test)]
mod finder_file_links_tests;
#[cfg(any(windows, test))]
mod launch_gate;
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
    let context = tauri::generate_context!();
    #[cfg(windows)]
    let launch_gate = launch_gate::acquire(&context.config().identifier);
    macro_rules! register {($(($name:ident,$arg:ty,$result:ty)),* $(,)?)=>{tauri::generate_handler![$($name),*]};}
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            file_open::deliver(app, file_open::parse_args(args, std::path::Path::new(&cwd)));
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(bindings::command_table!(register))
        .setup(move |app| {
            // Plugin setup has completed, so subsequent launches can forward their files.
            #[cfg(windows)]
            drop(launch_gate);
            let data = ariad_host::packs::data_dir()?;
            let backend =
                Backend::new(&data, std::env::current_exe()?).map_err(std::io::Error::other)?;
            app.manage(backend);
            let cwd = std::env::current_dir().unwrap_or_default();
            let launched = file_open::parse_args(std::env::args_os(), &cwd);
            file_open::deliver(app.handle(), launched);
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                app.deep_link().register_all()?;
            }
            Ok(())
        })
        .build(context)
        .expect("desktop runtime failed");
    app.run(|app, event| {
        // Finder, "Open With" and the Quick Action deliver files as Apple Events, not arguments.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = &event {
            file_open::deliver(app, file_open::parse_urls(urls));
        }
        if matches!(event, tauri::RunEvent::Exit) {
            app.state::<Arc<Backend>>().shutdown();
        }
    });
}
