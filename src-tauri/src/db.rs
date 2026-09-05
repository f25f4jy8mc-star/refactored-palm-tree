//! Opens the canonical model database and applies its schema.
//!
//! `001_model.sql` is the Phase 0 model document's schema, copied unchanged;
//! later files are additive and are applied in order behind it.
//!
//! Applying the whole set on every open used to be safe, because every
//! statement was `CREATE ... IF NOT EXISTS`. `003_view_shape.sql` is an
//! `ALTER TABLE`, which is not idempotent — so the app started once, wrote
//! the column, and aborted on the next launch with "duplicate column name".
//! `user_version` is now load-bearing, exactly as the note here warned it
//! would have to be.
//!
//! A database made before versions were tracked reads as 0 whatever it
//! actually contains, so the version is recovered from the schema itself the
//! first time. That covers all three cases in the wild at once: a fresh file,
//! one from before the column existed, and one from the build that added the
//! column without recording that it had.
//!
//! This file also turns on the pragmas the model relies on: foreign keys, so
//! `ON DELETE CASCADE` actually fires, and WAL, so a read from the UI doesn't
//! block a write from a scan.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations_model/001_model.sql"),
    include_str!("../migrations_model/002_sources.sql"),
    include_str!("../migrations_model/003_view_shape.sql"),
];

pub fn open(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("opening {}", db_path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&conn).with_context(|| format!("migrating {}", db_path.display()))?;
    Ok(conn)
}

/// Apply whatever this database has not had yet, and record how far it got.
pub fn migrate(conn: &Connection) -> Result<usize> {
    let from = applied_version(conn)?;
    let mut applied = 0;
    for (i, migration) in MIGRATIONS.iter().enumerate().skip(from) {
        conn.execute_batch(migration)
            .with_context(|| format!("migration {:03}", i + 1))?;
        applied += 1;
    }
    // A literal, because PRAGMA takes no parameters. The value is a length,
    // so there is nothing here a caller could inject.
    conn.execute_batch(&format!("PRAGMA user_version = {}", MIGRATIONS.len()))?;
    Ok(applied)
}

/// How many migrations this database has already had.
///
/// `user_version` when it has been set. When it is 0 the file may be new *or*
/// may predate version tracking, and those need opposite treatment — so the
/// schema is asked directly. Each check is for something a migration added,
/// in order, which is the only evidence available and is also exactly what
/// the migration would have created.
fn applied_version(conn: &Connection) -> Result<usize> {
    let recorded: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if recorded > 0 {
        return Ok((recorded as usize).min(MIGRATIONS.len()));
    }
    if !has_table(conn, "node")? {
        return Ok(0);
    }
    if !has_table(conn, "source")? {
        return Ok(1);
    }
    if !has_column(conn, "view_prefs", "shape")? {
        return Ok(2);
    }
    Ok(3)
}

fn has_table(conn: &Connection, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        rusqlite::params![name],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut q = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "archiva-db-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_database_can_be_opened_twice() {
        // The reported crash: the first launch wrote the column, the second
        // tried to add it again, `open` returned Err, and Tauri's setup error
        // became a panic across an FFI boundary — which aborts rather than
        // reporting anything useful.
        let dir = scratch();
        let path = dir.join("archiva-model.sqlite");
        drop(open(&path).expect("first open"));
        drop(open(&path).expect("second open"));
        drop(open(&path).expect("third open"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_fresh_database_gets_every_migration() {
        let dir = scratch();
        let path = dir.join("db.sqlite");
        let conn = open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
        assert!(has_column(&conn, "view_prefs", "shape").unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_database_from_before_the_column_is_upgraded_without_losing_anything() {
        let dir = scratch();
        let path = dir.join("db.sqlite");
        {
            // A library as an older build left it: the first two migrations,
            // no version recorded, and some of the user's work in it.
            let old = Connection::open(&path).unwrap();
            old.execute_batch(MIGRATIONS[0]).unwrap();
            old.execute_batch(MIGRATIONS[1]).unwrap();
            old.execute(
                "INSERT INTO node(id,node_type,content_type,display_name)
                 VALUES ('keep','media','public.jpeg','Harbour wall')",
                [],
            )
            .unwrap();
        }

        let conn = open(&path).unwrap();
        assert!(has_column(&conn, "view_prefs", "shape").unwrap());
        let name: String = conn
            .query_row("SELECT display_name FROM node WHERE id='keep'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(name, "Harbour wall", "an upgrade must not lose the library");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_database_the_broken_build_touched_is_recognised_rather_than_re_altered() {
        // It has the column but no recorded version, which is precisely the
        // state that crashed. Reading the version off the schema is what
        // tells the two apart.
        let dir = scratch();
        let path = dir.join("db.sqlite");
        {
            let old = Connection::open(&path).unwrap();
            for m in MIGRATIONS {
                old.execute_batch(m).unwrap();
            }
            old.execute_batch("PRAGMA user_version = 0").unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(applied_version(&conn).unwrap(), MIGRATIONS.len());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nothing_is_applied_a_second_time() {
        let dir = scratch();
        let path = dir.join("db.sqlite");
        {
            let conn = open(&path).unwrap();
            assert_eq!(migrate(&conn).unwrap(), 0, "already up to date");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_version_ahead_of_this_build_is_not_treated_as_work_to_do() {
        // Opening a library a newer build wrote. Nothing here can migrate it
        // forward, and re-running old migrations against it would be worse
        // than leaving it alone.
        let dir = scratch();
        let path = dir.join("db.sqlite");
        {
            let conn = open(&path).unwrap();
            conn.execute_batch("PRAGMA user_version = 99").unwrap();
        }
        let conn = Connection::open(&path).unwrap();
        assert_eq!(applied_version(&conn).unwrap(), MIGRATIONS.len());
        assert_eq!(migrate(&conn).unwrap(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
