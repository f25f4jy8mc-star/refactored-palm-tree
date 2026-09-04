//! The set of folders Archiva has been told to watch.
//!
//! Why this needs to exist as a table rather than being implied by the
//! nodes already indexed: `scan::scan` finishes by marking every
//! `local_file` node it did **not** see as `missing`. That is correct only
//! when the walk covered everywhere it should have — scanning one folder
//! while two are indexed would declare the other folder's files missing.
//! So a scan is always over *all* enabled sources, and this is the list
//! that makes "all" a knowable quantity.
//!
//! Removing a source deliberately leaves its nodes alone. Tags, links and
//! notes attached to those items are the user's work, and forgetting them
//! because a folder was unwatched would destroy far more than it tidies —
//! the items simply stop being refreshed, and go `missing` on the next scan
//! if they're really gone.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;

use super::scan::uuid_v7;

#[derive(Debug, Serialize)]
pub struct Source {
    pub id: String,
    pub path: String,
    pub enabled: bool,
    pub added_at: String,
    pub last_scan_at: Option<String>,
    /// Nodes whose locator sits under this path. Derived, never stored —
    /// a stored count is a second copy of something the nodes already know.
    pub item_count: i64,
}

pub fn list(conn: &Connection) -> Result<Vec<Source>> {
    let mut q = conn.prepare(
        "SELECT id, path, enabled, added_at, last_scan_at FROM source ORDER BY path",
    )?;
    let raw: Vec<(String, String, i64, String, Option<String>)> = q
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<std::result::Result<_, _>>()?;

    let mut out = Vec::with_capacity(raw.len());
    for (id, path, enabled, added_at, last_scan_at) in raw {
        // `LIKE path || '/%'` rather than a prefix match on `path` itself,
        // so /photos never counts /photos-old's items as its own.
        let item_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM node
              WHERE source_kind = 'local_file' AND locator LIKE ?1 || '/%'",
            params![path],
            |r| r.get(0),
        )?;
        out.push(Source {
            id,
            path,
            enabled: enabled != 0,
            added_at,
            last_scan_at,
            item_count,
        });
    }
    Ok(out)
}

/// Add a folder. Idempotent by path: adding one already watched returns the
/// existing row's id rather than a second row pointing at the same place.
pub fn add(conn: &Connection, path: &str) -> Result<String> {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return Err(anyhow!("a source needs a path"));
    }
    if let Ok(existing) = conn.query_row(
        "SELECT id FROM source WHERE path = ?1",
        params![path],
        |r| r.get::<_, String>(0),
    ) {
        // Re-adding a disabled source is how you turn it back on.
        conn.execute("UPDATE source SET enabled = 1 WHERE id = ?1", params![existing])?;
        return Ok(existing);
    }
    let id = uuid_v7();
    conn.execute(
        "INSERT INTO source(id, path, enabled) VALUES (?1, ?2, 1)",
        params![id, path],
    )?;
    Ok(id)
}

/// Stop watching. Indexed items stay — see the module header.
pub fn remove(conn: &Connection, id: &str) -> Result<()> {
    let n = conn.execute("DELETE FROM source WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(anyhow!("no such source: {id}"));
    }
    Ok(())
}

pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> Result<()> {
    let n = conn.execute(
        "UPDATE source SET enabled = ?2 WHERE id = ?1",
        params![id, i64::from(enabled)],
    )?;
    if n == 0 {
        return Err(anyhow!("no such source: {id}"));
    }
    Ok(())
}

/// Every enabled source's path, which is what a scan must walk in one pass
/// for its missing-sweep to mean anything.
pub fn enabled_roots(conn: &Connection) -> Result<Vec<PathBuf>> {
    let mut q = conn.prepare("SELECT path FROM source WHERE enabled = 1 ORDER BY path")?;
    let out = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    Ok(out)
}

pub fn mark_scanned(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE source SET last_scan_at = datetime('now') WHERE enabled = 1",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c.execute_batch(include_str!("../../migrations_model/002_sources.sql"))
            .unwrap();
        c
    }

    #[test]
    fn a_source_is_added_once_however_many_times_it_is_offered() {
        let c = db();
        let first = add(&c, "/photos").unwrap();
        let second = add(&c, "/photos").unwrap();
        assert_eq!(first, second);
        assert_eq!(list(&c).unwrap().len(), 1);
    }

    #[test]
    fn a_trailing_slash_is_not_a_different_folder() {
        let c = db();
        add(&c, "/photos").unwrap();
        add(&c, "/photos/").unwrap();
        assert_eq!(list(&c).unwrap().len(), 1);
    }

    #[test]
    fn re_adding_a_disabled_source_re_enables_it() {
        let c = db();
        let id = add(&c, "/photos").unwrap();
        set_enabled(&c, &id, false).unwrap();
        assert!(!list(&c).unwrap()[0].enabled);
        add(&c, "/photos").unwrap();
        assert!(list(&c).unwrap()[0].enabled);
    }

    #[test]
    fn only_enabled_sources_are_walked() {
        let c = db();
        add(&c, "/a").unwrap();
        let b = add(&c, "/b").unwrap();
        set_enabled(&c, &b, false).unwrap();
        let roots = enabled_roots(&c).unwrap();
        assert_eq!(roots, vec![PathBuf::from("/a")]);
    }

    #[test]
    fn removing_a_source_leaves_its_indexed_items_alone() {
        let c = db();
        let id = add(&c, "/photos").unwrap();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,locator)
             VALUES ('n1','media','public.jpeg','x','/photos/a.jpg')",
            [],
        )
        .unwrap();
        remove(&c, &id).unwrap();
        let left: i64 = c
            .query_row("SELECT COUNT(*) FROM node", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1, "unwatching a folder must not delete the user's work");
        assert!(list(&c).unwrap().is_empty());
    }

    #[test]
    fn item_counts_do_not_bleed_between_similarly_named_folders() {
        let c = db();
        add(&c, "/photos").unwrap();
        add(&c, "/photos-old").unwrap();
        for (id, loc) in [
            ("n1", "/photos/a.jpg"),
            ("n2", "/photos/b.jpg"),
            ("n3", "/photos-old/c.jpg"),
        ] {
            c.execute(
                "INSERT INTO node(id,node_type,content_type,display_name,locator)
                 VALUES (?1,'media','public.jpeg','x',?2)",
                params![id, loc],
            )
            .unwrap();
        }
        let sources = list(&c).unwrap();
        let photos = sources.iter().find(|s| s.path == "/photos").unwrap();
        let old = sources.iter().find(|s| s.path == "/photos-old").unwrap();
        assert_eq!(photos.item_count, 2);
        assert_eq!(old.item_count, 1);
    }

    #[test]
    fn removing_something_that_was_never_watched_is_an_error_not_a_silent_no_op() {
        let c = db();
        assert!(remove(&c, "nope").is_err());
        assert!(set_enabled(&c, "nope", false).is_err());
    }
}
