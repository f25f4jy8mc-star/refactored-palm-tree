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

use crate::ingest;
use crate::model::extract::RealExtractor;
use crate::model::projections::{self, Detail, ListOptions, ListPage};
use crate::model::scan;
use crate::model::search::{self, Hit};
use crate::model::sources::{self, Source};
use crate::model::tree::{self, Column};
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
pub fn tree_columns(db: State<Db>, path: Vec<String>) -> Result<Vec<Column>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    tree::tree(&conn, &path).map_err(|e| e.to_string())
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

/* ------------------------------------------------------------- sources */

#[tauri::command]
pub fn list_sources(db: State<Db>) -> Result<Vec<Source>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    sources::list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_source(app: AppHandle, db: State<Db>, path: String) -> Result<ScanReportDto, String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        sources::add(&conn, &path).map_err(|e| e.to_string())?;
    }
    rescan(app, db)
}

#[tauri::command]
pub fn remove_source(app: AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        sources::remove(&conn, &id).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("archiva:changed", ());
    Ok(())
}

#[tauri::command]
pub fn set_source_enabled(
    app: AppHandle,
    db: State<Db>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        sources::set_enabled(&conn, &id, enabled).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("archiva:changed", ());
    Ok(())
}

/// Re-index every enabled source, in one pass.
///
/// One pass over *all* of them, always — never a single folder. `scan`
/// finishes by marking every local file it didn't see as `missing`, so a
/// partial walk would declare the folders it skipped gone. That is why
/// there is no scan-this-one-folder command.
#[tauri::command]
pub fn rescan(app: AppHandle, db: State<Db>) -> Result<ScanReportDto, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let proxies_dir = data_dir.join("proxies");
    std::fs::create_dir_all(&proxies_dir).map_err(|e| e.to_string())?;
    let extractor = RealExtractor {
        proxies_dir,
        proxy_version: ingest::PROXY_VERSION,
    };

    let report = {
        let mut conn = db.0.lock().map_err(|e| e.to_string())?;
        let roots = sources::enabled_roots(&conn).map_err(|e| e.to_string())?;
        // Never index what Archiva itself writes (invariant 9) — the
        // workspace holds proxies and app-generated notes.
        let exclude: Vec<PathBuf> = vec![data_dir.clone()];
        let report =
            scan::scan(&mut conn, &roots, &exclude, &extractor).map_err(|e| e.to_string())?;
        sources::mark_scanned(&conn).map_err(|e| e.to_string())?;
        report
    };
    // One writer, one event — every open pane refetches independently rather
    // than the scanner knowing who currently has a stake in its result.
    let _ = app.emit("archiva:changed", ());
    Ok(ScanReportDto::from(report))
}

/* -------------------------------------------------------------- detail */

/// `p_detail`, plus the two fields a preview needs that `Row` doesn't
/// carry: where the original actually is, and the larger proxy. Both are
/// read here rather than widening the delivered projection.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailDto {
    #[serde(flatten)]
    pub detail: Detail,
    pub locator: Option<String>,
    pub preview_ref: Option<String>,
    pub size_bytes: Option<i64>,
}

#[tauri::command]
pub fn node_detail(db: State<Db>, id: String) -> Result<DetailDto, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let detail = projections::detail(&conn, &id, &projections::Options::default())
        .map_err(|e| e.to_string())?;
    let (locator, preview_ref, size_bytes) = conn
        .query_row(
            "SELECT locator, proxy_preview_ref, size_bytes FROM node WHERE id = ?1",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    Ok(DetailDto {
        detail,
        locator,
        preview_ref,
        size_bytes,
    })
}
