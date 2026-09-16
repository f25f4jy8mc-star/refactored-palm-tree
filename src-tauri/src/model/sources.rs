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
    /// What is in effect: whether this folder's contents are displayed.
    pub enabled: bool,
    /// What the tickbox says, when that differs from what is in effect.
    /// `None` means the two agree. Refresh is what closes the gap — see
    /// `apply_pending`.
    pub pending_enabled: Option<bool>,
    pub added_at: String,
    pub last_scan_at: Option<String>,
    /// Nodes whose locator sits under this path. Derived, never stored —
    /// a stored count is a second copy of something the nodes already know.
    pub item_count: i64,
}

pub fn list(conn: &Connection) -> Result<Vec<Source>> {
    let mut q = conn.prepare(
        "SELECT id, path, enabled, added_at, last_scan_at, pending_enabled
           FROM source ORDER BY path",
    )?;
    let raw: Vec<(String, String, i64, String, Option<String>, Option<i64>)> = q
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })?
        .collect::<std::result::Result<_, _>>()?;

    let mut out = Vec::with_capacity(raw.len());
    for (id, path, enabled, added_at, last_scan_at, pending) in raw {
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
            pending_enabled: pending.map(|p| p != 0),
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
        // Re-adding a disabled source is how you turn it back on — and it
        // takes effect now rather than waiting for a Refresh, because adding
        // a folder already scans, which is the same moment.
        conn.execute(
            "UPDATE source SET enabled = 1, pending_enabled = NULL WHERE id = ?1",
            params![existing],
        )?;
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

/// Tick or untick a folder. **Staged**: nothing about what is displayed
/// changes until `apply_pending` runs, which Refresh does.
///
/// Immediate would be easier to write and worse to use — half a library
/// disappearing while you are still working down a list of folders, with no
/// single moment you chose. Refresh is that moment, and it is where the
/// re-index happens anyway.
///
/// Setting it back to what is already in effect clears the pending change
/// rather than staging a no-op: unticking and reticking is not a change, and
/// a Refresh button lit for one would be lying.
pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> Result<()> {
    let n = conn.execute(
        "UPDATE source
            SET pending_enabled = CASE WHEN enabled = ?2 THEN NULL ELSE ?2 END
          WHERE id = ?1",
        params![id, i64::from(enabled)],
    )?;
    if n == 0 {
        return Err(anyhow!("no such source: {id}"));
    }
    Ok(())
}

/// Put every staged tickbox into effect. Returns how many actually changed.
pub fn apply_pending(conn: &Connection) -> Result<usize> {
    let changed = conn.execute(
        "UPDATE source SET enabled = pending_enabled WHERE pending_enabled IS NOT NULL",
        [],
    )?;
    conn.execute("UPDATE source SET pending_enabled = NULL", [])?;
    Ok(changed)
}

/// Whether anything is waiting for a Refresh. The panel reads this to say so.
pub fn has_pending(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM source WHERE pending_enabled IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Every enabled source's path, which is what a scan must walk in one pass
/// for its missing-sweep to mean anything.
/// What "switched off" means, once, as a predicate over a node aliased `n`.
///
/// A source you untick is not unwatched and its items are not forgotten:
/// they stop being *shown*. Tags, links and notes survive untouched, and
/// ticking it again brings the lot back — which is the difference between
/// hiding a folder and removing it, and the reason both exist.
///
/// `n.locator = s.path OR n.locator LIKE s.path || '/%'` rather than a bare
/// prefix, so switching off /photos never also hides /photos-old.
pub const HIDDEN_SQL: &str = "EXISTS (
        SELECT 1 FROM source s
         WHERE s.enabled = 0
           AND n.locator IS NOT NULL
           AND (n.locator = s.path OR n.locator LIKE s.path || '/%')
      )";

/// The nodes that predicate hides, for a reader that filters in Rust rather
/// than in SQL. The same sentence either way — there is one predicate.
pub fn hidden_ids(conn: &Connection) -> Result<std::collections::HashSet<String>> {
    // Nothing is switched off in most libraries, and asking first is cheaper
    // than walking every node to be told so.
    let off: i64 = conn.query_row("SELECT COUNT(*) FROM source WHERE enabled = 0", [], |r| {
        r.get(0)
    })?;
    if off == 0 {
        return Ok(Default::default());
    }
    let mut q = conn.prepare(&format!("SELECT n.id FROM node n WHERE {HIDDEN_SQL}"))?;
    let out = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

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
        // Every migration. A test database that is not the real schema is a
        // test that proves something else — `pending_enabled` lives in 004.
        crate::db::migrate(&c).unwrap();
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
        apply_pending(&c).unwrap();
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
        apply_pending(&c).unwrap();
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

    /* ------------------------------------------- switched off, not gone */

    fn indexed(c: &Connection, id: &str, locator: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,locator)
             VALUES (?1,'media','public.jpeg',?1,?2)",
            params![id, locator],
        )
        .unwrap();
    }

    #[test]
    fn switching_a_source_off_hides_what_is_under_it() {
        let c = db();
        let photos = add(&c, "/photos").unwrap();
        add(&c, "/scans").unwrap();
        indexed(&c, "a", "/photos/a.jpg");
        indexed(&c, "b", "/scans/b.jpg");

        assert!(hidden_ids(&c).unwrap().is_empty(), "nothing is off yet");
        set_enabled(&c, &photos, false).unwrap();
        apply_pending(&c).unwrap();
        let hidden = hidden_ids(&c).unwrap();
        assert!(hidden.contains("a"));
        assert!(!hidden.contains("b"), "the other folder is untouched");
    }

    #[test]
    fn hiding_a_folder_never_hides_the_one_whose_name_it_starts_with() {
        let c = db();
        let photos = add(&c, "/photos").unwrap();
        add(&c, "/photos-old").unwrap();
        indexed(&c, "new", "/photos/a.jpg");
        indexed(&c, "old", "/photos-old/b.jpg");

        set_enabled(&c, &photos, false).unwrap();
        apply_pending(&c).unwrap();
        let hidden = hidden_ids(&c).unwrap();
        assert!(hidden.contains("new"));
        assert!(!hidden.contains("old"), "/photos-old is a different folder");
    }

    #[test]
    fn switching_it_back_on_brings_everything_back() {
        // Hidden, not forgotten: the rows were never touched, so the tags,
        // links and notes on them are exactly where they were.
        let c = db();
        let photos = add(&c, "/photos").unwrap();
        indexed(&c, "a", "/photos/a.jpg");

        set_enabled(&c, &photos, false).unwrap();
        apply_pending(&c).unwrap();
        assert_eq!(hidden_ids(&c).unwrap().len(), 1);
        let still: i64 = c
            .query_row("SELECT COUNT(*) FROM node WHERE id = 'a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(still, 1, "the item is still in the library");

        set_enabled(&c, &photos, true).unwrap();
        apply_pending(&c).unwrap();
        assert!(hidden_ids(&c).unwrap().is_empty());
    }

    #[test]
    fn unticking_stages_the_change_and_refresh_applies_it() {
        // Half a library disappearing while you are still working down a
        // list of folders, with no single moment you chose, is why this is
        // staged. Refresh is that moment.
        let c = db();
        let photos = add(&c, "/photos").unwrap();
        indexed(&c, "a", "/photos/a.jpg");

        set_enabled(&c, &photos, false).unwrap();
        assert!(has_pending(&c).unwrap(), "the panel can say a Refresh is due");
        assert_eq!(list(&c).unwrap()[0].pending_enabled, Some(false));
        assert!(list(&c).unwrap()[0].enabled, "and nothing is in effect yet");
        assert!(hidden_ids(&c).unwrap().is_empty(), "so nothing is hidden yet");

        assert_eq!(apply_pending(&c).unwrap(), 1);
        assert!(!has_pending(&c).unwrap());
        assert_eq!(list(&c).unwrap()[0].pending_enabled, None);
        assert!(!list(&c).unwrap()[0].enabled);
        assert_eq!(hidden_ids(&c).unwrap().len(), 1);
    }

    #[test]
    fn unticking_and_reticking_is_not_a_change() {
        // A Refresh button lit for a no-op would be lying about there being
        // something to apply.
        let c = db();
        let photos = add(&c, "/photos").unwrap();
        set_enabled(&c, &photos, false).unwrap();
        set_enabled(&c, &photos, true).unwrap();
        assert!(!has_pending(&c).unwrap());
        assert_eq!(list(&c).unwrap()[0].pending_enabled, None);
        assert_eq!(apply_pending(&c).unwrap(), 0);
    }

    #[test]
    fn a_staged_folder_is_still_walked_until_the_refresh_that_drops_it() {
        // Refresh applies the tickboxes *then* walks, so the folder being
        // switched off is not scanned on the way out.
        let c = db();
        let a = add(&c, "/a").unwrap();
        add(&c, "/b").unwrap();
        set_enabled(&c, &a, false).unwrap();
        assert_eq!(enabled_roots(&c).unwrap().len(), 2, "not applied yet");
        apply_pending(&c).unwrap();
        assert_eq!(
            enabled_roots(&c).unwrap(),
            vec![PathBuf::from("/b")],
            "and after Refresh applies it, only /b is walked"
        );
    }

    #[test]
    fn re_adding_a_folder_clears_a_staged_removal() {
        // Adding a folder scans, which is the same moment a Refresh would
        // be — so "I want this watched" takes effect rather than queueing
        // behind a tick you already changed your mind about.
        let c = db();
        let photos = add(&c, "/photos").unwrap();
        set_enabled(&c, &photos, false).unwrap();
        add(&c, "/photos").unwrap();
        assert!(!has_pending(&c).unwrap());
        assert!(list(&c).unwrap()[0].enabled);
    }

    #[test]
    fn something_that_came_from_nowhere_on_disk_is_never_hidden() {
        // A board you made here has no locator, so no folder can switch it
        // off — it does not belong to one.
        let c = db();
        let photos = add(&c, "/photos").unwrap();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name)
             VALUES ('board','collector','app.archiva.collector.board','My board')",
            [],
        )
        .unwrap();
        set_enabled(&c, &photos, false).unwrap();
        apply_pending(&c).unwrap();
        assert!(hidden_ids(&c).unwrap().is_empty());
    }
}
