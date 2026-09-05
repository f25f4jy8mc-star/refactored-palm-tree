//! Taking items out of the library.
//!
//! Until now there was no way to remove anything, and the gap showed: turning
//! a source off leaves its items behind on purpose (their tags, links and
//! notes are the user's work), so unwatching every folder and re-indexing left
//! a library full of items marked `missing` and no way to clear them.
//!
//! Two different acts, kept apart because confusing them loses files:
//!
//!   * **forget** — the rows go, the files do not. Everything Archiva knew
//!     about the item is gone; the bytes on disk are untouched. If the file is
//!     still inside a watched folder the next scan will index it again, as a
//!     new item with a new id and none of its tags. That is not a bug, it is
//!     what "forget, don't delete" means, and the interface says so.
//!   * **trash** — the file is moved into Archiva's own trash folder first,
//!     and then forgotten. It is out of every watched folder, so it does not
//!     come back, and it is still on disk if it was a mistake.
//!
//! Nothing here deletes a file outright. The trash folder lives inside the app
//! workspace, which the scanner excludes by construction, so a trashed file is
//! invisible to the index without being destroyed.
//!
//! Deleting is not undoable within the app — `reconcile_log` records what the
//! scanner decided, not what the user did, and there is no undo stack yet
//! (checklist: ⌘Z is listed as deferred). So the count of what is about to go
//! is returned before anything happens, and the interface shows it.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removal {
    /// Nodes whose rows were deleted.
    pub forgotten: usize,
    /// Files moved into the trash folder.
    pub trashed: usize,
    /// Files that could not be moved — the node is kept, so nothing is lost
    /// silently. A partial failure must not read as a success.
    pub failed: Vec<String>,
}

/// What is about to go, before anything goes.
///
/// A collector's members are *not* counted: removing a folder removes the
/// folder, and the things it gathered stay in the library. Anything else would
/// make deleting one collector a way to lose a hundred photographs.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub items: usize,
    pub collectors: usize,
    pub notes: usize,
    /// Members that will be released from the collectors being removed.
    pub released: usize,
    /// Files that exist on disk and could be trashed.
    pub with_files: usize,
}

pub fn preview(conn: &Connection, ids: &[String]) -> Result<Preview> {
    let mut out = Preview::default();
    for id in ids {
        let row: Option<(String, Option<String>, String)> = conn
            .query_row(
                "SELECT node_type, locator, source_kind FROM node WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((node_type, locator, source_kind)) = row else {
            continue;
        };
        match node_type.as_str() {
            "collector" => {
                out.collectors += 1;
                let members: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM edge WHERE target_id = ?1 AND kind = 'contains'",
                    params![id],
                    |r| r.get(0),
                )?;
                out.released += members as usize;
            }
            "note" => out.notes += 1,
            _ => out.items += 1,
        }
        if source_kind == "local_file" {
            if let Some(path) = locator {
                if Path::new(&path).exists() {
                    out.with_files += 1;
                }
            }
        }
    }
    Ok(out)
}

/// Delete the rows. Files are never touched.
///
/// Edges, tags, attributes, notes and the search index all cascade from
/// `node`, so one DELETE is the whole job — that is what the schema's
/// ON DELETE CASCADE is for. The reconcile log deliberately does not cascade:
/// it records what happened, and must outlive the thing it happened to.
pub fn forget(conn: &Connection, ids: &[String]) -> Result<usize> {
    let mut n = 0;
    for id in ids {
        n += conn.execute("DELETE FROM node WHERE id = ?1", params![id])?;
    }
    Ok(n)
}

/// Move each item's file into `trash_dir`, then forget it.
///
/// A node whose file cannot be moved is **kept**, and named in `failed`. The
/// alternative — forgetting it anyway — would leave a file in a watched folder
/// that reappears on the next scan, which reads as the delete having silently
/// failed.
pub fn trash(conn: &Connection, ids: &[String], trash_dir: &Path) -> Result<Removal> {
    std::fs::create_dir_all(trash_dir)?;
    let mut out = Removal::default();
    let mut forgettable: Vec<String> = Vec::new();

    for id in ids {
        let row: Option<(Option<String>, String, String)> = conn
            .query_row(
                "SELECT locator, source_kind, display_name FROM node WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((locator, source_kind, name)) = row else {
            continue;
        };

        // Nothing on disk to move: a collector, a tag, a remote item, or a
        // file already gone. Forgetting it is the whole operation.
        let path = match (source_kind.as_str(), locator.as_deref()) {
            ("local_file", Some(p)) if Path::new(p).exists() => PathBuf::from(p),
            _ => {
                forgettable.push(id.clone());
                continue;
            }
        };

        match move_into(&path, trash_dir, id) {
            Ok(()) => {
                out.trashed += 1;
                forgettable.push(id.clone());
            }
            Err(e) => out.failed.push(format!("{name}: {e}")),
        }
    }

    out.forgotten = forget(conn, &forgettable)?;
    Ok(out)
}

/// Prefixed with the node id, so two files called `IMG_4821.jpg` from
/// different folders cannot overwrite each other in the trash — and so a
/// trashed file can still be traced back to the row it belonged to.
fn move_into(path: &Path, trash_dir: &Path, id: &str) -> Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let target = trash_dir.join(format!("{id}__{name}"));

    // Rename first: it is atomic and instant within one filesystem. It fails
    // across a mount boundary, which is exactly when a copy is required — a
    // watched folder on an external drive is the ordinary case, not an edge
    // case.
    if std::fs::rename(path, &target).is_ok() {
        return Ok(());
    }
    std::fs::copy(path, &target)?;
    std::fs::remove_file(path)
        // The copy landed but the original would not go. Removing the copy
        // keeps the two in step rather than leaving a duplicate behind.
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&target);
        })?;
    Ok(())
}

/// Empty the library: every node, and the scan history with it.
///
/// This is the "start again" button, and the one place the reconcile log is
/// cleared — a history of decisions about items that no longer exist has
/// nothing to explain. Sources are kept, so the next scan refills from the
/// folders still being watched; clearing while folders are still watched is
/// how you get the same library back, which the interface warns about.
pub fn clear_library(conn: &Connection) -> Result<usize> {
    let n = conn.execute("DELETE FROM node", [])?;
    conn.execute("DELETE FROM reconcile_log", [])?;
    // Suggestions dismissed against ids that no longer exist would sit in the
    // table forever, never matching anything.
    conn.execute("DELETE FROM dismissed", [])?;
    Ok(n)
}

/// Forget everything indexed under one path. Used when a source is removed
/// and the user asks for its items to go with it.
pub fn forget_under(conn: &Connection, root: &str) -> Result<usize> {
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return Err(anyhow!("refusing to clear items under an empty path"));
    }
    // `LIKE root || '/%'` rather than a prefix on `root` itself, so /photos
    // never takes /photos-old's items with it.
    Ok(conn.execute(
        "DELETE FROM node
          WHERE source_kind = 'local_file' AND locator LIKE ?1 || '/%'",
        params![root],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::scan::uuid_v7;
    use crate::model::tags;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archiva-removal-{}", uuid_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn item(c: &Connection, id: &str, locator: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,filename,locator)
             VALUES (?1,'media','public.jpeg',?1,?1,?2)",
            params![id, locator],
        )
        .unwrap();
    }

    fn collector(c: &Connection, id: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name)
             VALUES (?1,'collector','app.archiva.virtual',?1)",
            params![id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id, collector_kind) VALUES (?1,'folder')",
            params![id],
        )
        .unwrap();
    }

    fn count(c: &Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn forgetting_takes_the_row_and_everything_hanging_off_it() {
        let c = db();
        item(&c, "a", "/photos/a.jpg");
        let t = tags::ensure(&c, "coast", "environment").unwrap();
        tags::apply(&c, &["a".into()], &t).unwrap();
        c.execute(
            "INSERT INTO attribute(node_id,key,value) VALUES ('a','width','4032')",
            [],
        )
        .unwrap();

        assert_eq!(forget(&c, &["a".into()]).unwrap(), 1);
        assert_eq!(count(&c, "attribute"), 0, "attributes cascade");
        assert_eq!(count(&c, "edge"), 0, "the tag_of edge cascades");
        assert_eq!(count(&c, "search"), 1, "the tag's own search row remains");
        // The tag itself is vocabulary, not a property of the item.
        assert_eq!(tags::list(&c).unwrap().len(), 1);
    }

    #[test]
    fn forgetting_never_touches_the_file() {
        let dir = scratch();
        let path = dir.join("a.jpg");
        std::fs::write(&path, b"x").unwrap();
        let c = db();
        item(&c, "a", path.to_str().unwrap());

        forget(&c, &["a".into()]).unwrap();
        assert!(path.exists(), "forget must never delete a file");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn removing_a_collector_releases_its_members_rather_than_taking_them() {
        let c = db();
        collector(&c, "col");
        item(&c, "a", "/photos/a.jpg");
        c.execute(
            "INSERT INTO edge(id,source_id,target_id,kind) VALUES ('e','a','col','contains')",
            [],
        )
        .unwrap();

        let p = preview(&c, &["col".into()]).unwrap();
        assert_eq!(p.collectors, 1);
        assert_eq!(p.released, 1);

        forget(&c, &["col".into()]).unwrap();
        let left: i64 = c
            .query_row("SELECT COUNT(*) FROM node WHERE id = 'a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1, "deleting a folder must not delete what it held");
        assert_eq!(count(&c, "edge"), 0, "but the membership goes");
    }

    #[test]
    fn trashing_moves_the_file_out_of_the_watched_folder() {
        let dir = scratch();
        let watched = dir.join("photos");
        let bin = dir.join("trash");
        std::fs::create_dir_all(&watched).unwrap();
        let path = watched.join("a.jpg");
        std::fs::write(&path, b"x").unwrap();

        let c = db();
        item(&c, "a", path.to_str().unwrap());
        let out = trash(&c, &["a".into()], &bin).unwrap();

        assert_eq!(out.trashed, 1);
        assert_eq!(out.forgotten, 1);
        assert!(out.failed.is_empty());
        assert!(!path.exists(), "gone from the watched folder");
        assert_eq!(
            std::fs::read_dir(&bin).unwrap().count(),
            1,
            "and still on disk"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_trashed_file_keeps_its_id_so_two_of_one_name_cannot_collide() {
        let dir = scratch();
        let bin = dir.join("trash");
        let c = db();
        for (id, sub) in [("a", "one"), ("b", "two")] {
            let folder = dir.join(sub);
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join("IMG_4821.jpg");
            std::fs::write(&path, id.as_bytes()).unwrap();
            item(&c, id, path.to_str().unwrap());
        }
        let out = trash(&c, &["a".into(), "b".into()], &bin).unwrap();
        assert_eq!(out.trashed, 2);
        assert_eq!(
            std::fs::read_dir(&bin).unwrap().count(),
            2,
            "same filename, two files kept"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_item_with_no_file_is_simply_forgotten() {
        let c = db();
        collector(&c, "col");
        item(&c, "gone", "/definitely/not/here.jpg");
        let bin = scratch().join("trash");
        let out = trash(&c, &["col".into(), "gone".into()], &bin).unwrap();
        assert_eq!(out.trashed, 0);
        assert_eq!(out.forgotten, 2);
        assert!(out.failed.is_empty());
    }

    #[test]
    fn clearing_empties_the_library_and_its_history_but_keeps_the_sources() {
        let c = db();
        c.execute_batch(include_str!("../../migrations_model/002_sources.sql"))
            .unwrap();
        crate::model::sources::add(&c, "/photos").unwrap();
        item(&c, "a", "/photos/a.jpg");
        c.execute(
            "INSERT INTO reconcile_log(node_id, table_version, signals, rule)
             VALUES ('a', 1, 2, 13)",
            [],
        )
        .unwrap();

        assert_eq!(clear_library(&c).unwrap(), 1);
        assert_eq!(count(&c, "node"), 0);
        assert_eq!(count(&c, "reconcile_log"), 0);
        assert_eq!(
            crate::model::sources::list(&c).unwrap().len(),
            1,
            "the folders are still watched — clearing is not unwatching"
        );
    }

    #[test]
    fn forgetting_under_a_path_does_not_reach_a_similarly_named_folder() {
        let c = db();
        item(&c, "a", "/photos/a.jpg");
        item(&c, "b", "/photos/deep/b.jpg");
        item(&c, "c", "/photos-old/c.jpg");
        assert_eq!(forget_under(&c, "/photos").unwrap(), 2);
        let left: String = c
            .query_row("SELECT id FROM node", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, "c");
    }

    #[test]
    fn forgetting_under_an_empty_path_is_refused_rather_than_clearing_everything() {
        let c = db();
        item(&c, "a", "/photos/a.jpg");
        assert!(forget_under(&c, "").is_err());
        assert!(forget_under(&c, "/").is_err());
        assert_eq!(count(&c, "node"), 1);
    }

    #[test]
    fn a_file_that_cannot_be_moved_keeps_its_row() {
        // The trash path is a file, so creating it as a directory fails and
        // every move fails with it. Nothing may be forgotten on that basis.
        let dir = scratch();
        let watched = dir.join("photos");
        std::fs::create_dir_all(&watched).unwrap();
        let path = watched.join("a.jpg");
        std::fs::write(&path, b"x").unwrap();
        let blocked = dir.join("not-a-dir");
        std::fs::write(&blocked, b"x").unwrap();

        let c = db();
        item(&c, "a", path.to_str().unwrap());
        let err = trash(&c, &["a".into()], &blocked);
        assert!(err.is_err(), "the trash folder could not be made");
        assert_eq!(count(&c, "node"), 1, "and nothing was forgotten");
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
