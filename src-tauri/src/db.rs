//! Opens the canonical model database and applies its schema.
//!
//! The schema itself is `migrations_model/001_model.sql`, copied unchanged
//! from the Phase 0 model document. This file only knows how to find it and
//! turn the pragmas on that the model relies on (foreign keys, so
//! `ON DELETE CASCADE` actually fires; WAL, so a read from the UI doesn't
//! block a write from a scan).

use std::path::Path;

use rusqlite::Connection;

const SCHEMA: &str = include_str!("../migrations_model/001_model.sql");

pub fn open(db_path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}
