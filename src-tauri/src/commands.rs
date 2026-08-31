//! The Tauri command surface the frontend calls.
//!
//! Deliberately thin: every command deserialises its arguments, hands them to
//! `model::*` unchanged, and serialises the result. No query logic lives here
//! — that would be a second copy of what the projection already decides, and
//! two copies is exactly the class of bug this rebuild exists to remove.

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::model::extract::RealExtractor;
use crate::model::projections::{self, ListOptions, ListPage};
use crate::model::scan;
use crate::model::search::{self, Hit};
use crate::model::view_prefs::{self, ViewPrefs};

pub struct Db(pub Mutex<Connection>);

fn default_group_by() -> String {
    "type".into()
}
fn default_sort() -> String {
    "name".into()
}

/// Mirrors `model::projections::ListOptions`, field for field, so the model
/// module stays exactly as delivered and only this DTO knows about JSON.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRowsArgs {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default = "default_group_by")]
    pub group_by: String,
    #[serde(default = "default_sort")]
    pub sort: String,
    #[serde(default)]
    pub descending: bool,
    #[serde(default)]
    pub expanded: Vec<String>,
    #[serde(default)]
    pub query: Option<String>,
}

impl From<ListRowsArgs> for ListOptions {
    fn from(a: ListRowsArgs) -> Self {
        ListOptions {
            scope: a.scope,
            group_by: a.group_by,
            sort: a.sort,
            descending: a.descending,
            expanded: a.expanded,
            query: a.query,
        }
    }
}

#[tauri::command]
pub fn list_rows(db: State<Db>, args: ListRowsArgs) -> Result<ListPage, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    projections::rows(&conn, &args.into()).map_err(|e| e.to_string())
}

/// `model::scan::ScanReport` carries no `Serialize` impl — the model crate is
/// copied unchanged, so the mapping happens here rather than there.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReportDto {
    pub seen: usize,
    pub created: usize,
    pub updated: usize,
    pub touched: usize,
    pub refreshed: usize,
    pub deferred: usize,
    pub unreadable: usize,
    pub went_missing: usize,
}

impl From<scan::ScanReport> for ScanReportDto {
    fn from(r: scan::ScanReport) -> Self {
        Self {
            seen: r.seen,
            created: r.created,
            updated: r.updated,
            touched: r.touched,
            refreshed: r.refreshed,
            deferred: r.deferred,
            unreadable: r.unreadable,
            went_missing: r.went_missing,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchArgs {
    pub query: String,
    #[serde(default)]
    pub type_filter: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}
fn default_search_limit() -> usize {
    50
}

#[tauri::command]
pub fn search_library(db: State<Db>, args: SearchArgs) -> Result<Vec<Hit>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let opts = search::Options {
        type_filter: args.type_filter,
        limit: args.limit,
    };
    search::search(&conn, &args.query, &opts).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_view_prefs(db: State<Db>, scope_id: String, pane_kind: String) -> Result<ViewPrefs, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    view_prefs::get(&conn, &scope_id, &pane_kind).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_view_prefs(
    db: State<Db>,
    scope_id: String,
    pane_kind: String,
    prefs: ViewPrefs,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    view_prefs::set(&conn, &scope_id, &pane_kind, &prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn scan_folder(app: AppHandle, db: State<Db>, path: String) -> Result<ScanReportDto, String> {
    let proxies_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("proxies");
    std::fs::create_dir_all(&proxies_dir).map_err(|e| e.to_string())?;
    let extractor = RealExtractor {
        proxies_dir,
        proxy_version: 1,
    };

    let report = {
        let mut conn = db.0.lock().map_err(|e| e.to_string())?;
        let root = PathBuf::from(path);
        scan::scan(&mut conn, &[root], &[], &extractor).map_err(|e| e.to_string())?
    };
    // One writer, one event — every open pane refetches independently rather
    // than the scanner knowing who currently has a stake in its result.
    let _ = app.emit("archiva:changed", ());
    Ok(ScanReportDto::from(report))
}
