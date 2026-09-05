//! Opens the canonical model database and applies its schema.
//!
//! `001_model.sql` is the Phase 0 model document's schema, copied unchanged;
//! later files are additive and are applied in order behind it. Every
//! statement in each is `CREATE ... IF NOT EXISTS`, so applying the whole
//! set on every open is idempotent and needs no version bookkeeping yet —
//! when a migration first has to *alter* something, that stops being true
//! and `user_version` becomes load-bearing.
//!
//! This file also turns on the pragmas the model relies on: foreign keys,
//! so `ON DELETE CASCADE` actually fires, and WAL, so a read from the UI
//! doesn't block a write from a scan.

use std::path::Path;

use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations_model/001_model.sql"),
    include_str!("../migrations_model/002_sources.sql"),
    include_str!("../migrations_model/003_view_shape.sql"),
];

pub fn open(db_path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    for migration in MIGRATIONS {
        conn.execute_batch(migration)?;
    }
    Ok(conn)
}
