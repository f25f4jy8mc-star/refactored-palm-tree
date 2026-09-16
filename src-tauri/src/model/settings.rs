//! A space's own preferences.
//!
//! Not `view_prefs`, which remembers how one *pane* was last looking at one
//! scope (§1.9, G13). These are decisions about the whole library — true
//! wherever you look at it, and the same for every pane and every window.
//!
//! Key/value so a new setting needs no migration, the same reasoning
//! `attribute` uses in §1.4. Values are text; whether one is a boolean is
//! decided here, once, rather than by each caller parsing for itself.
//!
//! Every setting has a default, and reading an unset key returns it. A
//! library that has never been told what to do should behave, not refuse.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Whether the folders you linked are drawn in the Library as items.
///
/// Off by default. The Library is a listing of what you *have*, and a
/// mirrored folder is a description of where some of it sits — useful to see
/// sometimes, noise the rest of the time, so it is a choice rather than a
/// fact. Collectors you made here are unaffected either way: those are
/// gatherings, and a gathering is something you have.
pub const SHOW_LINKED_FOLDERS: &str = "show_linked_folders";

/// Every setting, with the defaults filled in. One read for the frontend,
/// so a view never has to know which keys exist.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub show_linked_folders: bool,
}

pub fn all(conn: &Connection) -> Result<Settings> {
    Ok(Settings {
        show_linked_folders: flag(conn, SHOW_LINKED_FOLDERS, false)?,
    })
}

/// A stored boolean, or `default` when nothing has been stored.
pub fn flag(conn: &Connection, key: &str, default: bool) -> Result<bool> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM setting WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match stored.as_deref() {
        Some("1") | Some("true") => true,
        Some("0") | Some("false") => false,
        // Anything else is a value this build does not understand, which is
        // not a reason to fail — the default is still the honest answer.
        _ => default,
    })
}

pub fn set_flag(conn: &Connection, key: &str, on: bool) -> Result<()> {
    conn.execute(
        "INSERT INTO setting(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, if on { "1" } else { "0" }],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::db::migrate(&c).unwrap();
        c
    }

    #[test]
    fn an_unset_setting_is_its_default_rather_than_an_error() {
        let c = db();
        assert!(!all(&c).unwrap().show_linked_folders);
        assert!(flag(&c, "never-heard-of-it", true).unwrap());
    }

    #[test]
    fn what_was_set_is_what_comes_back() {
        let c = db();
        set_flag(&c, SHOW_LINKED_FOLDERS, true).unwrap();
        assert!(all(&c).unwrap().show_linked_folders);
        set_flag(&c, SHOW_LINKED_FOLDERS, false).unwrap();
        assert!(!all(&c).unwrap().show_linked_folders);
    }

    #[test]
    fn setting_it_twice_keeps_one_row() {
        let c = db();
        set_flag(&c, SHOW_LINKED_FOLDERS, true).unwrap();
        set_flag(&c, SHOW_LINKED_FOLDERS, true).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM setting", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn a_value_this_build_does_not_understand_reads_as_the_default() {
        // A newer build's value, or a hand-edited database. Refusing to draw
        // the library over it would be worse than drawing it the usual way.
        let c = db();
        c.execute(
            "INSERT INTO setting(key,value) VALUES (?1,'sometimes')",
            params![SHOW_LINKED_FOLDERS],
        )
        .unwrap();
        assert!(!all(&c).unwrap().show_linked_folders);
    }
}
