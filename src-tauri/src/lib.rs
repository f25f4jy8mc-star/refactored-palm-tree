mod commands;
mod db;
mod ingest;
mod model;

use commands::Db;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = db::open(&data_dir.join("archiva-model.sqlite"))?;
            app.manage(Db(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_rows,
            commands::search_library,
            commands::get_view_prefs,
            commands::set_view_prefs,
            commands::tree_columns,
            commands::node_detail,
            commands::list_sources,
            commands::add_source,
            commands::remove_source,
            commands::set_source_enabled,
            commands::rescan,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
