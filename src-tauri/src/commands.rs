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
use crate::model::facets::{self, Facet};
use crate::model::health;
use crate::model::identity::{self, Recheck};
use crate::model::projections::{self, Detail, ListOptions, ListPage};
use crate::model::record::{self, Record};
use crate::model::folders;
use crate::model::rowtree;
use crate::model::removal::{self, Preview, Removal};
use crate::model::scan;
use crate::model::search::{self, Hit};
use crate::model::sources::{self, Source};
use crate::model::suggest::{self, DuplicatePair};
use crate::model::tags::{self, Tag};
use crate::model::tree::{self, Column};
use crate::model::view_prefs::{self, ViewPrefs};

pub struct Db(pub Mutex<Connection>);

fn default_group_by() -> String {
    "type".into()
}
fn default_sort() -> String {
    "name".into()
}
fn default_shape() -> String {
    "source".into()
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
    /// `source` — every item once, folders left out. `hierarchy` — the tree,
    /// rooted at what nothing contains, nesting an expanded folder's members
    /// however deep they go. Two ways of reading one library, and the pane
    /// says which it wants rather than each guessing.
    #[serde(default = "default_shape")]
    pub shape: String,
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
    let shape = args.shape.clone();
    let opts: ListOptions = args.into();
    match shape.as_str() {
        "hierarchy" => rowtree::hierarchy(&conn, &opts),
        _ => rowtree::source(&conn, &opts),
    }
    .map_err(|e| e.to_string())
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

/// `p_tree` — the Miller cascade, starting at `root` (a collector id) or at
/// the library root when it is None. A pane scoped to a folder nested inside
/// another needs to start there: that folder is not in the library's root
/// column, so a walk that began with it stopped before it started.
///
/// `workspace` is the Viewer's root: the watched folders are not drawn, their
/// contents are. The Library's Hierarchy asks for the other one, where the
/// disk scaffolding is the point. It is decided per caller rather than in the
/// projection because it is a question about the pane, not about the data.
#[tauri::command]
pub fn tree_columns(
    db: State<Db>,
    root: Option<String>,
    path: Vec<String>,
    workspace: Option<bool>,
) -> Result<Vec<Column>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let cascade = if workspace.unwrap_or(false) {
        tree::workspace
    } else {
        tree::tree_from
    };
    cascade(&conn, root.as_deref(), &path).map_err(|e| e.to_string())
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

/// Stop watching a folder.
///
/// `forget_items` decides what happens to what was indexed from it. The
/// default is to keep them — their tags, links and notes are the user's work
/// and unwatching a folder should not throw that away. Passing true is the
/// answer to "I want this gone", and it forgets rows only: the files are
/// never touched by this call.
#[tauri::command]
pub fn remove_source(
    app: AppHandle,
    db: State<Db>,
    id: String,
    forget_items: bool,
) -> Result<usize, String> {
    let forgotten = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let path: String = conn
            .query_row(
                "SELECT path FROM source WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        sources::remove(&conn, &id).map_err(|e| e.to_string())?;
        let forgotten = if forget_items {
            removal::forget_under(&conn, &path).map_err(|e| e.to_string())?
        } else {
            0
        };
        // Folders that held only what just went are structure describing
        // nothing, so they go too.
        let roots = sources::enabled_roots(&conn).map_err(|e| e.to_string())?;
        folders::rebuild(&conn, &roots).map_err(|e| e.to_string())?;
        forgotten
    };
    let _ = app.emit("archiva:changed", ());
    Ok(forgotten)
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
        // The scan records where each file is but creates nothing to stand
        // for the folders themselves, so the hierarchy is built here from
        // what it wrote. Without this the library is a flat pile and every
        // hierarchy surface — the tree, Miller columns, opening a collector —
        // has nothing to show.
        folders::rebuild(&conn, &roots).map_err(|e| e.to_string())?;
        // The scan can only say present or missing. This is where missing is
        // refined into permission_denied, and where a drive plugged back in
        // stops being badged (G1).
        identity::recheck(&conn).map_err(|e| e.to_string())?;
        // Titles and tag counts move when items are created, renamed or go
        // missing, so the parts are recomputed once here rather than by each
        // view working them out for itself (G20).
        health::recompute_all(&conn).map_err(|e| e.to_string())?;
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

/* -------------------------------------------------------------- record */

/// `p_record` — everything known about one item. The Inspector's single read.
#[tauri::command]
pub fn node_record(db: State<Db>, id: String) -> Result<Record, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    record::record(&conn, &id).map_err(|e| e.to_string())
}

/* ------------------------------------------------------ classification */

/// The facet vocabulary. Static, but served from the backend so the frontend
/// never holds a second copy of which tier a facet belongs to — that pair is
/// already denormalised once in the database and a third copy in TypeScript
/// is how the three drift.
#[tauri::command]
pub fn list_facets() -> Vec<&'static Facet> {
    facets::FACETS.iter().collect()
}

#[tauri::command]
pub fn list_tags(db: State<Db>) -> Result<Vec<Tag>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    tags::list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_tag(app: AppHandle, db: State<Db>, name: String, facet: String) -> Result<String, String> {
    let id = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        tags::ensure(&conn, &name, &facet).map_err(|e| e.to_string())?
    };
    let _ = app.emit("archiva:changed", ());
    Ok(id)
}

/// Apply one tag to a whole selection. Batch is the default shape, not an
/// optimisation — see `model::tags`.
#[tauri::command]
pub fn apply_tag(
    app: AppHandle,
    db: State<Db>,
    node_ids: Vec<String>,
    tag_id: String,
) -> Result<usize, String> {
    let changed = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let n = tags::apply(&conn, &node_ids, &tag_id).map_err(|e| e.to_string())?;
        health::recompute_many(&conn, &node_ids).map_err(|e| e.to_string())?;
        n
    };
    let _ = app.emit("archiva:changed", ());
    Ok(changed)
}

#[tauri::command]
pub fn remove_tag(
    app: AppHandle,
    db: State<Db>,
    node_ids: Vec<String>,
    tag_id: String,
) -> Result<usize, String> {
    let changed = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let n = tags::unapply(&conn, &node_ids, &tag_id).map_err(|e| e.to_string())?;
        health::recompute_many(&conn, &node_ids).map_err(|e| e.to_string())?;
        n
    };
    let _ = app.emit("archiva:changed", ());
    Ok(changed)
}

#[tauri::command]
pub fn rename_tag(app: AppHandle, db: State<Db>, tag_id: String, name: String) -> Result<(), String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        tags::rename(&conn, &tag_id, &name).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("archiva:changed", ());
    Ok(())
}

#[tauri::command]
pub fn set_tag_facet(
    app: AppHandle,
    db: State<Db>,
    tag_id: String,
    facet: String,
) -> Result<(), String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        tags::set_facet(&conn, &tag_id, &facet).map_err(|e| e.to_string())?;
        // Every item carrying it just changed which facet it has filled.
        health::recompute_all(&conn).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("archiva:changed", ());
    Ok(())
}

/// Deleting or merging a tag changes the health of items this call has no
/// other way of naming, so both recompute everything rather than guessing.
#[tauri::command]
pub fn delete_tag(app: AppHandle, db: State<Db>, tag_id: String) -> Result<usize, String> {
    let carried = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let n = tags::delete(&conn, &tag_id).map_err(|e| e.to_string())?;
        health::recompute_all(&conn).map_err(|e| e.to_string())?;
        n
    };
    let _ = app.emit("archiva:changed", ());
    Ok(carried)
}

#[tauri::command]
pub fn merge_tags(
    app: AppHandle,
    db: State<Db>,
    from: String,
    into: String,
) -> Result<usize, String> {
    let moved = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let n = tags::merge(&conn, &from, &into).map_err(|e| e.to_string())?;
        health::recompute_all(&conn).map_err(|e| e.to_string())?;
        n
    };
    let _ = app.emit("archiva:changed", ());
    Ok(moved)
}

#[tauri::command]
pub fn reorder_tag(app: AppHandle, db: State<Db>, tag_id: String, to: i64) -> Result<(), String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        tags::reorder(&conn, &tag_id, to).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("archiva:changed", ());
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotedDto {
    pub collector_id: String,
    pub moved: usize,
}

#[tauri::command]
pub fn promote_tag(
    app: AppHandle,
    db: State<Db>,
    tag_id: String,
    name: Option<String>,
    strip_tag: bool,
) -> Result<PromotedDto, String> {
    let out = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let p = tags::promote_to_collector(&conn, &tag_id, name.as_deref(), strip_tag)
            .map_err(|e| e.to_string())?;
        health::recompute_all(&conn).map_err(|e| e.to_string())?;
        PromotedDto {
            collector_id: p.collector_id,
            moved: p.moved,
        }
    };
    let _ = app.emit("archiva:changed", ());
    Ok(out)
}

/* --------------------------------------------------------- suggestions */

#[tauri::command]
pub fn duplicate_tags(db: State<Db>) -> Result<Vec<DuplicatePair>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    suggest::near_duplicates(&conn).map_err(|e| e.to_string())
}

/// Accept a proposed Format or Era: make the tag if it does not exist, then
/// apply it. Accept-only is the rule — nothing here can be reached except by
/// a person clicking accept.
#[tauri::command]
pub fn accept_suggestion(
    app: AppHandle,
    db: State<Db>,
    node_id: String,
    facet: String,
    name: String,
) -> Result<String, String> {
    let tag_id = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let tag_id = tags::ensure(&conn, &name, &facet).map_err(|e| e.to_string())?;
        tags::apply(&conn, &[node_id.clone()], &tag_id).map_err(|e| e.to_string())?;
        health::recompute(&conn, &node_id).map_err(|e| e.to_string())?;
        tag_id
    };
    let _ = app.emit("archiva:changed", ());
    Ok(tag_id)
}

#[tauri::command]
pub fn dismiss_suggestion(
    app: AppHandle,
    db: State<Db>,
    key: String,
    kind: String,
) -> Result<(), String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        suggest::dismiss(&conn, &key, &kind).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("archiva:changed", ());
    Ok(())
}

/* --------------------------------------------------- source and reach */

/// Add an item that lives at a URL. It arrives `remote_uncached`: the row
/// exists, nothing has been fetched, and no view reports it as broken.
#[tauri::command]
pub fn add_remote_item(
    app: AppHandle,
    db: State<Db>,
    url: String,
    title: Option<String>,
) -> Result<String, String> {
    let id = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let id = identity::add_remote(&conn, &url, title.as_deref()).map_err(|e| e.to_string())?;
        health::recompute(&conn, &id).map_err(|e| e.to_string())?;
        id
    };
    let _ = app.emit("archiva:changed", ());
    Ok(id)
}

/// Re-examine everything not currently present, without a full walk.
#[tauri::command]
pub fn recheck_availability(app: AppHandle, db: State<Db>) -> Result<Recheck, String> {
    let out = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        identity::recheck(&conn).map_err(|e| e.to_string())?
    };
    let _ = app.emit("archiva:changed", ());
    Ok(out)
}

/* ------------------------------------------------------------ removal */

/// What is about to go, before anything goes. There is no undo yet, so the
/// interface shows this and waits.
#[tauri::command]
pub fn preview_removal(db: State<Db>, ids: Vec<String>) -> Result<Preview, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    removal::preview(&conn, &ids).map_err(|e| e.to_string())
}

/// Remove items from the library.
///
/// `trash_files` false forgets the rows and leaves every file alone — which
/// means a file still inside a watched folder comes back on the next scan, as
/// a new item with none of its tags. True moves each file into Archiva's own
/// trash folder first, which is inside the workspace the scanner excludes, so
/// it stays gone and stays recoverable.
#[tauri::command]
pub fn delete_items(
    app: AppHandle,
    db: State<Db>,
    ids: Vec<String>,
    trash_files: bool,
) -> Result<Removal, String> {
    let trash_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("trash");
    let out = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        if trash_files {
            removal::trash(&conn, &ids, &trash_dir).map_err(|e| e.to_string())?
        } else {
            Removal {
                forgotten: removal::forget(&conn, &ids).map_err(|e| e.to_string())?,
                ..Default::default()
            }
        }
    };
    let _ = app.emit("archiva:changed", ());
    Ok(out)
}

/// Empty the library. Watched folders are kept, so the next scan refills from
/// whatever is still being watched — the interface says so before asking.
#[tauri::command]
pub fn clear_library(app: AppHandle, db: State<Db>) -> Result<usize, String> {
    let n = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        removal::clear_library(&conn).map_err(|e| e.to_string())?
    };
    let _ = app.emit("archiva:changed", ());
    Ok(n)
}
