-- Watched sources — layer 00 of the architecture ("watched folders +
-- workspace"), which 001 never gave a home to: it models what a node *is*
-- and where it was found, but not the set of places Archiva has been told
-- to look. Without that set there is no honest answer to "rescan", because
-- a scan of one folder can't tell a file that is genuinely gone from a file
-- that simply belongs to a folder this pass didn't walk.
--
-- Additive. 001 is untouched; both files are applied in order at startup,
-- every statement being CREATE ... IF NOT EXISTS.

CREATE TABLE IF NOT EXISTS source (
  id           TEXT PRIMARY KEY,           -- UUID v7, same convention as node
  path         TEXT NOT NULL,              -- absolute directory path
  enabled      INTEGER NOT NULL DEFAULT 1,
  added_at     TEXT NOT NULL DEFAULT (datetime('now')),
  last_scan_at TEXT
);

-- One row per path. Not a table-level UNIQUE on a nullable column (see the
-- header of 001) — `path` is NOT NULL, so a plain unique index is safe here.
CREATE UNIQUE INDEX IF NOT EXISTS idx_source_path ON source(path);
CREATE INDEX IF NOT EXISTS idx_source_enabled ON source(enabled) WHERE enabled = 1;
