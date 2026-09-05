//! The folder structure on disk, made into Collectors.
//!
//! `scan` records where every file *is* — `parent_dir` is on each node — but
//! it creates nothing to represent the folders themselves. So a freshly
//! indexed library is a flat pile: no hierarchy to expand, nothing for Miller
//! columns to cascade through, and nothing for "open this collector in the
//! Viewer" to open. Every hierarchy feature was built against a shape the
//! indexer never produced.
//!
//! This pass builds it, after the scan and from what the scan wrote. A folder
//! becomes a Collector like any other, and membership is a `contains` edge
//! like any other — the same mechanism a Collector you make by hand uses, so
//! nothing downstream has to know where a grouping came from.
//!
//! Two details that matter:
//!
//!   * A folder Collector is `app_generated`, not `local_file`. `scan` ends by
//!     marking every `local_file` node it did not walk past as `missing`, and
//!     it walks files only — so a folder recorded as a local file would go
//!     missing on the very scan that created it.
//!   * Folders that no longer hold anything are removed here, not left to
//!     accumulate. A directory you emptied should stop being offered, and the
//!     items are gone by then anyway.
//!
//! Idempotent: running it twice changes nothing, which is the same property
//! the scanner's own second pass has and for the same reason.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use super::content_type;
use super::scan::uuid_v7;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderReport {
    pub created: usize,
    pub linked: usize,
    pub removed: usize,
}

/// Rebuild the folder hierarchy for everything indexed under `roots`.
pub fn rebuild(conn: &Connection, roots: &[PathBuf]) -> Result<FolderReport> {
    let mut report = FolderReport::default();
    if roots.is_empty() {
        report.removed = prune(conn)?;
        return Ok(report);
    }

    // Every indexed file, with the directory it sits in.
    let mut q = conn.prepare(
        "SELECT id, parent_dir FROM node
          WHERE source_kind = 'local_file' AND parent_dir IS NOT NULL",
    )?;
    let items: Vec<(String, String)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    drop(q);

    // One lookup of what already exists, rather than a query per file.
    let mut known = existing_folders(conn)?;

    for (node_id, dir) in items {
        let path = PathBuf::from(&dir);
        let Some(root) = roots.iter().find(|r| path.starts_with(r)) else {
            // Indexed from a folder no longer watched. Leave it alone — the
            // item is still the user's; it simply has no hierarchy now.
            continue;
        };
        let folder_id = ensure_chain(conn, &path, root, &mut known, &mut report)?;
        if link(conn, &node_id, &folder_id)? {
            report.linked += 1;
        }
    }

    report.removed = prune(conn)?;
    Ok(report)
}

/// Make sure a Collector exists for `dir` and for every directory between it
/// and `root`, each contained by its parent. Returns the id for `dir`.
fn ensure_chain(
    conn: &Connection,
    dir: &Path,
    root: &Path,
    known: &mut HashMap<String, String>,
    report: &mut FolderReport,
) -> Result<String> {
    // Root first, then down: a folder can only be linked to a parent that
    // already exists.
    let mut chain: Vec<&Path> = Vec::new();
    let mut at = Some(dir);
    while let Some(p) = at {
        chain.push(p);
        if p == root {
            break;
        }
        at = p.parent();
    }
    chain.reverse();

    let mut parent: Option<String> = None;
    for p in chain {
        let key = p.to_string_lossy().to_string();
        let id = match known.get(&key) {
            Some(id) => id.clone(),
            None => {
                let id = create(conn, p, root)?;
                known.insert(key, id.clone());
                report.created += 1;
                id
            }
        };
        if let Some(parent_id) = parent {
            if link(conn, &id, &parent_id)? {
                report.linked += 1;
            }
        }
        parent = Some(id);
    }
    // The chain always holds at least `dir` itself, so this is unreachable —
    // but it runs inside a Tauri command, where a panic crosses an FFI
    // boundary and aborts the process rather than surfacing anything. An
    // error is the honest shape for something that cannot happen.
    parent.ok_or_else(|| anyhow!("no folder chain for {}", dir.display()))
}

fn create(conn: &Connection, dir: &Path, root: &Path) -> Result<String> {
    // A watched root shows its own last component rather than "/" — the name
    // you chose the folder by is the name you look for.
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());
    let subtitle = if dir == root { "watched folder" } else { "folder" };

    // The folder *type*, not the generic virtual one. `expand` is granted at
    // `app.archiva.collector`, so a folder typed as merely virtual conforms
    // to nothing that grants it — and a folder that cannot expand is a folder
    // the Miller cascade will not open and the tree will not descend into.
    const FOLDER: &str = "app.archiva.collector.folder";

    let id = uuid_v7();
    conn.execute(
        "INSERT INTO node(id, node_type, content_type, content_type_tree, title,
                          source_kind, locator, display_name, display_subtitle, icon_kind,
                          tagging_health, title_quality)
         VALUES (?1, 'collector', ?2, ?3, ?4, 'app_generated', ?5, ?4, ?6, 'folder', 3, 1)",
        params![
            id,
            FOLDER,
            serde_json::to_string(&content_type::closure(FOLDER))?,
            name,
            dir.to_string_lossy(),
            subtitle,
        ],
    )?;
    conn.execute(
        "INSERT INTO collector(node_id, collector_kind) VALUES (?1, 'folder')",
        params![id],
    )?;
    Ok(id)
}

/// `source` is contained by `target`. Returns whether the edge was new.
fn link(conn: &Connection, source: &str, target: &str) -> Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO edge(id, source_id, target_id, kind, origin)
         VALUES (?1, ?2, ?3, 'contains', 'extension')",
        params![uuid_v7(), source, target],
    )?;
    Ok(n > 0)
}

fn existing_folders(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut q = conn.prepare(
        "SELECT locator, id FROM node
          WHERE node_type = 'collector' AND source_kind = 'app_generated'
            AND locator IS NOT NULL",
    )?;
    let out = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<HashMap<_, _>, _>>()?;
    Ok(out)
}

/// Remove folder Collectors that hold nothing, repeatedly, so emptying a deep
/// branch removes the whole branch rather than its innermost folder only.
///
/// Only ones this module made: a Collector you created by hand stays whether
/// or not it is empty, because you meant it to exist.
fn prune(conn: &Connection) -> Result<usize> {
    let mut removed = 0;
    loop {
        let n = conn.execute(
            "DELETE FROM node
              WHERE node_type = 'collector'
                AND source_kind = 'app_generated'
                AND locator IS NOT NULL
                AND id NOT IN (SELECT target_id FROM edge
                                WHERE kind = 'contains' AND target_id IS NOT NULL)",
            [],
        )?;
        if n == 0 {
            break;
        }
        removed += n;
    }
    Ok(removed)
}

/// The ids of every folder this module maintains, for a view that wants to
/// tell a folder from a gathering.
pub fn derived_ids(conn: &Connection) -> Result<HashSet<String>> {
    let mut q = conn.prepare(
        "SELECT id FROM node
          WHERE node_type = 'collector' AND source_kind = 'app_generated'
            AND locator IS NOT NULL",
    )?;
    let out = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::scan::{self, Extractor, Proxies};
    use std::time::{Duration, SystemTime};

    /// Measures nothing and makes no proxies — this module's business is
    /// structure, and a real extractor would only slow the test down.
    struct Bare;
    impl Extractor for Bare {
        fn extract(&self, _p: &Path, _ct: &str) -> Vec<(String, Option<String>, Option<f64>)> {
            vec![]
        }
        fn version(&self) -> i64 {
            1
        }
        fn proxies(&self, _p: &Path, _ct: &str, _h: Option<&str>) -> Proxies {
            Proxies::not_applicable(1)
        }
    }

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archiva-folders-{}", uuid_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Files must be older than the scanner's quiet window or rule 1 defers
    /// them as still being written.
    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(60)).unwrap();
    }

    /// A real scan of a real directory tree, then this pass. Nothing is
    /// hand-inserted: the point is what the indexer actually produces.
    fn indexed(dir: &Path) -> Connection {
        let mut c = db();
        scan::scan(&mut c, &[dir.to_path_buf()], &[], &Bare).unwrap();
        rebuild(&c, &[dir.to_path_buf()]).unwrap();
        c
    }

    fn names_with_parents(c: &Connection) -> Vec<(String, String)> {
        let mut q = c
            .prepare(
                "SELECT child.display_name, parent.display_name
                   FROM edge e
                   JOIN node child ON child.id = e.source_id
                   JOIN node parent ON parent.id = e.target_id
                  WHERE e.kind = 'contains'
                  ORDER BY parent.display_name, child.display_name",
            )
            .unwrap();
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn a_scanned_tree_becomes_a_hierarchy() {
        let dir = scratch();
        write(&dir.join("alpha.jpg"), b"a");
        write(&dir.join("Trips/photo.jpg"), b"b");
        write(&dir.join("Trips/Bergamo/deep.jpg"), b"c");

        let c = indexed(&dir);
        let root = dir.file_name().unwrap().to_string_lossy().to_string();
        let mut pairs = names_with_parents(&c);
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("Bergamo".to_string(), "Trips".to_string()),
                ("Trips".to_string(), root.clone()),
                ("alpha".to_string(), root),
                ("deep".to_string(), "Bergamo".to_string()),
                ("photo".to_string(), "Trips".to_string()),
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_folder_can_be_expanded() {
        // Without this the Miller cascade will not open it and the tree will
        // not descend: `expand` is granted at `app.archiva.collector`, and a
        // folder typed as merely virtual conforms to nothing that grants it.
        let dir = scratch();
        write(&dir.join("Trips/photo.jpg"), b"b");
        let c = indexed(&dir);
        let id: String = c
            .query_row(
                "SELECT id FROM node WHERE display_name = 'Trips'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let row = crate::model::projections::row(&c, &id).unwrap();
        assert!(
            row.capabilities.iter().any(|cap| cap == "expand"),
            "a folder must be expandable — got {:?}",
            row.capabilities
        );
        assert_eq!(row.node_type, "collector");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn running_it_twice_changes_nothing() {
        let dir = scratch();
        write(&dir.join("Trips/photo.jpg"), b"b");
        let c = indexed(&dir);
        let before = names_with_parents(&c);
        let nodes: i64 = c
            .query_row("SELECT COUNT(*) FROM node", [], |r| r.get(0))
            .unwrap();

        let again = rebuild(&c, &[dir.clone()]).unwrap();
        assert_eq!(again.created, 0);
        assert_eq!(again.linked, 0);
        assert_eq!(again.removed, 0);
        assert_eq!(names_with_parents(&c), before);
        assert_eq!(
            c.query_row::<i64, _, _>("SELECT COUNT(*) FROM node", [], |r| r.get(0))
                .unwrap(),
            nodes
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn folders_are_app_generated_so_the_scan_never_marks_them_missing() {
        // The trap this module would otherwise fall into: `sweep_missing`
        // badges every local_file node the walk did not see, and the walk
        // sees files only.
        let dir = scratch();
        write(&dir.join("Trips/photo.jpg"), b"b");
        let mut c = indexed(&dir);
        scan::scan(&mut c, &[dir.clone()], &[], &Bare).unwrap();

        let badged: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM node WHERE node_type='collector' AND availability<>'present'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(badged, 0, "a folder must not go missing on the next scan");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_emptied_branch_is_removed_all_the_way_up() {
        let dir = scratch();
        write(&dir.join("Trips/Bergamo/deep.jpg"), b"c");
        write(&dir.join("keep.jpg"), b"k");
        let mut c = indexed(&dir);
        assert_eq!(
            c.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM node WHERE node_type='collector'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            3
        );

        // Delete the only file in the branch and re-index.
        std::fs::remove_file(dir.join("Trips/Bergamo/deep.jpg")).unwrap();
        scan::scan(&mut c, &[dir.clone()], &[], &Bare).unwrap();
        c.execute("DELETE FROM node WHERE availability = 'missing'", [])
            .unwrap();
        rebuild(&c, &[dir.clone()]).unwrap();

        let left: Vec<String> = {
            let mut q = c
                .prepare("SELECT display_name FROM node WHERE node_type='collector' ORDER BY display_name")
                .unwrap();
            q.query_map([], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            left,
            vec![dir.file_name().unwrap().to_string_lossy().to_string()],
            "Trips and Bergamo both went, not just the innermost one"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_collector_made_by_hand_survives_being_empty() {
        let dir = scratch();
        write(&dir.join("a.jpg"), b"a");
        let c = indexed(&dir);
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,source_kind)
             VALUES ('mine','collector','app.archiva.virtual','My board','app_generated')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id, collector_kind) VALUES ('mine','board')",
            [],
        )
        .unwrap();

        rebuild(&c, &[dir.clone()]).unwrap();
        let still: i64 = c
            .query_row("SELECT COUNT(*) FROM node WHERE id='mine'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(still, 1, "it has no locator, so it is not one of ours");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unwatching_keeps_the_hierarchy_of_the_items_that_remain() {
        // Removing a source keeps its items by default, and where those items
        // sit is still true. Tearing the folders down would leave the items
        // in a flat pile for no reason.
        let dir = scratch();
        write(&dir.join("Trips/photo.jpg"), b"b");
        let c = indexed(&dir);
        let out = rebuild(&c, &[]).unwrap();
        assert_eq!(out.removed, 0);
        assert_eq!(
            c.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM node WHERE node_type='collector'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            2
        );

        // Forget the items, and the folders go with them — they were only
        // ever a description of where those items were.
        c.execute("DELETE FROM node WHERE node_type = 'media'", []).unwrap();
        let after = rebuild(&c, &[]).unwrap();
        assert_eq!(after.removed, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_watched_roots_do_not_share_a_hierarchy() {
        let a = scratch();
        let b = scratch();
        write(&a.join("one.jpg"), b"1");
        write(&b.join("two.jpg"), b"2");
        let mut c = db();
        scan::scan(&mut c, &[a.clone(), b.clone()], &[], &Bare).unwrap();
        rebuild(&c, &[a.clone(), b.clone()]).unwrap();

        let roots: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM node n
                  WHERE n.node_type='collector'
                    AND NOT EXISTS (SELECT 1 FROM edge e
                                     WHERE e.source_id = n.id AND e.kind='contains')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(roots, 2, "one top-level folder per watched root");
        std::fs::remove_dir_all(&a).ok();
        std::fs::remove_dir_all(&b).ok();
    }
}
