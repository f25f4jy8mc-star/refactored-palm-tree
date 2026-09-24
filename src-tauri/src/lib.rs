mod commands;
mod db;
mod ingest;
mod model;

use commands::Db;
use std::sync::Mutex;
use tauri::Manager;

/// Open the space that was open last, if there still is one.
///
/// Neither a missing space nor an unreachable one is a startup failure: a
/// first run has none, and a space on a drive that is not plugged in is an
/// ordinary Tuesday. Both arrive at the same place — a window with no space
/// open, which is a screen, not an error.
fn start(app: &tauri::App) -> anyhow::Result<()> {
    let app_data = app.path().app_data_dir()?;
    std::fs::create_dir_all(&app_data)?;

    let open = match model::spaces::current(&app_data)? {
        Some(space) => match model::spaces::open(&app_data, &space.id) {
            Ok((space, conn)) => Some(commands::Open { space, conn }),
            Err(e) => {
                eprintln!("The last space could not be opened: {e:#}");
                None
            }
        },
        None => None,
    };

    app.manage(Db {
        open: Mutex::new(open),
        app_data,
    });
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
            commands::note_body,
            commands::list_spaces,
            commands::current_space,
            commands::open_space,
            commands::create_space,
            commands::open_space_folder,
            commands::rename_space,
            commands::move_space,
            commands::forget_space,
            commands::list_sources,
            commands::add_source,
            commands::remove_source,
            commands::set_source_enabled,
            commands::rescan,
            commands::add_to_arm,
            commands::unlink_edge,
            commands::gather_target,
            commands::gather,
            commands::ungather,
            commands::rows_of,
            commands::selection_tags,
            commands::create_item,
            commands::get_settings,
            commands::set_show_linked_folders,
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
