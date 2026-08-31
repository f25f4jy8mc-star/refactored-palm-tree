//! Per-scope view memory (§1.9, G13) — Finder-parity ⌘1/2/3.
//!
//! One row per (scope, pane): a folder remembers how it was last looked at,
//! and that's user data, not interaction state — it belongs here, not in the
//! frontend's Zustand-equivalent. `scope_id` is a collector id or a
//! well-known scope name ("library", "scattered") for the views that aren't
//! scoped to one folder.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ViewPrefs {
    pub layout: Option<String>,
    pub sort: Option<String>,
    pub group_by: Option<String>,
    pub density: Option<String>,
}

pub fn get(conn: &Connection, scope_id: &str, pane_kind: &str) -> Result<ViewPrefs> {
    Ok(conn
        .query_row(
            "SELECT layout, sort, group_by, density FROM view_prefs
              WHERE scope_id = ?1 AND pane_kind = ?2",
            params![scope_id, pane_kind],
            |r| {
                Ok(ViewPrefs {
                    layout: r.get(0)?,
                    sort: r.get(1)?,
                    group_by: r.get(2)?,
                    density: r.get(3)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn set(conn: &Connection, scope_id: &str, pane_kind: &str, prefs: &ViewPrefs) -> Result<()> {
    conn.execute(
        "INSERT INTO view_prefs(scope_id, pane_kind, layout, sort, group_by, density, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
         ON CONFLICT(scope_id, pane_kind) DO UPDATE SET
           layout = ?3, sort = ?4, group_by = ?5, density = ?6, updated_at = datetime('now')",
        params![
            scope_id,
            pane_kind,
            prefs.layout,
            prefs.sort,
            prefs.group_by,
            prefs.density,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    #[test]
    fn an_unset_scope_reads_as_all_none() {
        let c = seed();
        let p = get(&c, "library", "library").unwrap();
        assert!(p.layout.is_none() && p.sort.is_none() && p.group_by.is_none());
    }

    #[test]
    fn a_saved_scope_is_read_back_exactly() {
        let c = seed();
        let prefs = ViewPrefs {
            layout: Some("grid".into()),
            sort: Some("captured".into()),
            group_by: Some("health".into()),
            density: None,
        };
        set(&c, "library", "library", &prefs).unwrap();
        let back = get(&c, "library", "library").unwrap();
        assert_eq!(back.layout.as_deref(), Some("grid"));
        assert_eq!(back.sort.as_deref(), Some("captured"));
        assert_eq!(back.group_by.as_deref(), Some("health"));
    }

    #[test]
    fn saving_twice_updates_rather_than_duplicating() {
        let c = seed();
        set(&c, "k1", "library", &ViewPrefs { layout: Some("list".into()), ..Default::default() }).unwrap();
        set(&c, "k1", "library", &ViewPrefs { layout: Some("grid".into()), ..Default::default() }).unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM view_prefs", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        assert_eq!(get(&c, "k1", "library").unwrap().layout.as_deref(), Some("grid"));
    }

    #[test]
    fn two_scopes_are_independent() {
        let c = seed();
        set(&c, "k1", "library", &ViewPrefs { layout: Some("grid".into()), ..Default::default() }).unwrap();
        set(&c, "k2", "library", &ViewPrefs { layout: Some("list".into()), ..Default::default() }).unwrap();
        assert_eq!(get(&c, "k1", "library").unwrap().layout.as_deref(), Some("grid"));
        assert_eq!(get(&c, "k2", "library").unwrap().layout.as_deref(), Some("list"));
    }

    #[test]
    fn two_panes_on_the_same_scope_are_independent() {
        let c = seed();
        set(&c, "k1", "library", &ViewPrefs { layout: Some("grid".into()), ..Default::default() }).unwrap();
        set(&c, "k1", "scattered", &ViewPrefs { layout: Some("list".into()), ..Default::default() }).unwrap();
        assert_eq!(get(&c, "k1", "library").unwrap().layout.as_deref(), Some("grid"));
        assert_eq!(get(&c, "k1", "scattered").unwrap().layout.as_deref(), Some("list"));
    }
}
