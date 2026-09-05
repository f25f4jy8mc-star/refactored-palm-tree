mod commands;
mod db;
mod ingest;
mod model;

use commands::Db;
use std::sync::Mutex;
use tauri::Manager;

fn start(app: &tauri::App) -> anyhow::Result<()> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let conn = db::open(&data_dir.join("archiva-model.sqlite"))?;
    app.manage(Db(Mutex::new(conn)));
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Say what went wrong here, while there is still somewhere to say
            // it. A setup error becomes a panic inside an OS callback, and a
            // panic there cannot unwind — the process aborts and the real
            // cause is lost above a backtrace of the panic printer itself.
            match start(app) {
                Ok(()) => Ok(()),
                Err(e) => {
                    eprintln!("Archiva could not start: {e:#}");
                    Err(e.into())
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_rows,
            commands::search_library,
            commands::get_view_prefs,
            commands::set_view_prefs,
            commands::tree_columns,
            commands::node_detail,
            commands::node_record,
            commands::list_sources,
            commands::add_source,
            commands::remove_source,
            commands::set_source_enabled,
            commands::rescan,
            commands::list_facets,
            commands::list_tags,
            commands::create_tag,
            commands::apply_tag,
            commands::remove_tag,
            commands::rename_tag,
            commands::set_tag_facet,
            commands::delete_tag,
            commands::merge_tags,
            commands::reorder_tag,
            commands::promote_tag,
            commands::duplicate_tags,
            commands::accept_suggestion,
            commands::dismiss_suggestion,
            commands::add_remote_item,
            commands::recheck_availability,
            commands::preview_removal,
            commands::delete_items,
            commands::clear_library,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            // Exiting beats panicking: the panic would cross the same FFI
            // boundary and abort with the cause buried.
            eprintln!("Archiva stopped: {e}");
            std::process::exit(1);
        });
}
