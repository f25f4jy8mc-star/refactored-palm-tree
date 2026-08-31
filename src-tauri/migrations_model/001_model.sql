-- Archiva — the canonical model, from archiva-phase-0-model.md.
--
-- This schema owns its own database file (archiva-model.sqlite). It shares
-- nothing with the v1 database: no shared tables, no shared names, no
-- possibility of a query reaching the wrong set. The old file keeps running
-- Build 17 untouched, and is deleted when the last view has been converted.
--
-- Versioned independently, starting at 1. The v1 file's user_version stays
-- where it is; the two never interact.
--
-- Two conventions carried through the whole file:
--   * Ids are UUID v7 stored as TEXT. Identity is independent of content,
--     path, filename and title — every one of those is mutable. (§1.1)
--   * Never a table-level UNIQUE containing a nullable column. SQLite treats
--     NULLs as distinct so the constraint silently never fires; that is what
--     migration 002 of the old schema existed to repair. Partial unique
--     indexes throughout.

-- ---------------------------------------------------------------- nodes

CREATE TABLE IF NOT EXISTS node (
  id                 TEXT PRIMARY KEY,          -- UUID v7: sorts by creation time
  node_type          TEXT NOT NULL CHECK (node_type IN ('media','note','collector','tag')),
  content_type       TEXT NOT NULL,             -- UTI-style leaf, e.g. 'public.jpeg'
  content_type_tree  TEXT NOT NULL DEFAULT '[]',-- JSON array, leaf first, materialised closure
  title              TEXT NOT NULL DEFAULT '',

  -- source and availability (§1.2)
  source_kind        TEXT NOT NULL DEFAULT 'local_file'
                       CHECK (source_kind IN ('local_file','remote_url','app_generated')),
  locator            TEXT,                      -- absolute path, or URL; NULL for tags/collectors
  parent_dir         TEXT,
  filename           TEXT,
  extension          TEXT,
  inode              INTEGER,
  device             INTEGER,
  size_bytes         INTEGER,
  content_hash       TEXT,                      -- BLAKE3. An attribute, never identity.
  mtime              TEXT,
  ctime              TEXT,
  availability       TEXT NOT NULL DEFAULT 'present'
                       CHECK (availability IN ('present','missing','remote_uncached','permission_denied')),
  last_seen_at       TEXT,

  -- proxies (§1.3) — four artefacts, not one
  proxy_thumb_ref    TEXT,
  proxy_preview_ref  TEXT,
  proxy_playable_ref TEXT,
  proxy_version      INTEGER NOT NULL DEFAULT 0,
  proxy_state        TEXT NOT NULL DEFAULT 'not_applicable'
                       CHECK (proxy_state IN ('not_applicable','pending','ready','failed')),

  -- derived (§1.6). Recomputed by the indexer; never written by a view.
  display_name       TEXT NOT NULL DEFAULT '',
  display_subtitle   TEXT NOT NULL DEFAULT '',
  icon_kind          TEXT NOT NULL DEFAULT '',
  tagging_health     INTEGER NOT NULL DEFAULT 0 CHECK (tagging_health BETWEEN 0 AND 3),
  facets_filled      INTEGER NOT NULL DEFAULT 0,   -- health components, kept separate (G20)
  title_quality      INTEGER NOT NULL DEFAULT 0,
  has_any_tag        INTEGER NOT NULL DEFAULT 0,
  unresolved_links   INTEGER NOT NULL DEFAULT 0,

  created_at         TEXT NOT NULL DEFAULT (datetime('now')),
  indexed_at         TEXT NOT NULL DEFAULT (datetime('now')),
  modified_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Reconciliation lookups, in the order the ladder tries them (rules 6–13).
CREATE INDEX IF NOT EXISTS idx_node_inode   ON node(device, inode) WHERE inode IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_node_hash    ON node(content_hash) WHERE content_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_node_locator ON node(locator) WHERE locator IS NOT NULL;

-- Deliberately NOT unique on content_hash. Two files may legitimately hold the
-- same bytes (rule 12, "copied"), and the v1 unique index on file_hash is the
-- reason editing a note produced a second node.
CREATE INDEX IF NOT EXISTS idx_node_type    ON node(node_type, content_type);
CREATE INDEX IF NOT EXISTS idx_node_avail   ON node(availability) WHERE availability <> 'present';
CREATE INDEX IF NOT EXISTS idx_node_health  ON node(tagging_health);
CREATE INDEX IF NOT EXISTS idx_node_added   ON node(indexed_at);   -- date-added views (§9.2)

-- One node per path. Partial, because virtual nodes have no locator.
CREATE UNIQUE INDEX IF NOT EXISTS idx_node_path_unique
  ON node(locator) WHERE locator IS NOT NULL AND source_kind = 'local_file';

-- ------------------------------------------------------- kind-specific

CREATE TABLE IF NOT EXISTS tag (
  node_id     TEXT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
  facet       TEXT NOT NULL CHECK (facet IN
                ('format','era','environment','action','attribute','subject','unclassified')),
  tier        INTEGER NOT NULL CHECK (tier BETWEEN 0 AND 3),
  sort_order  INTEGER NOT NULL DEFAULT 0,
  -- Denormalised on purpose (facet determines tier), so tier filters are an
  -- index lookup. Checked rather than trusted:
  CHECK ((facet = 'unclassified' AND tier = 0)
      OR (facet IN ('format','era') AND tier = 1)
      OR (facet IN ('environment','action') AND tier = 2)
      OR (facet IN ('attribute','subject') AND tier = 3))
);
CREATE INDEX IF NOT EXISTS idx_tag_facet ON tag(facet, sort_order);

CREATE TABLE IF NOT EXISTS collector (
  node_id              TEXT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
  collector_kind       TEXT NOT NULL CHECK (collector_kind IN ('folder','board')),
  promoted_from_tag_id TEXT REFERENCES node(id) ON DELETE SET NULL,
  viewport_x           REAL NOT NULL DEFAULT 0,   -- boards: last canvas position
  viewport_y           REAL NOT NULL DEFAULT 0,
  viewport_zoom        REAL NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS note (
  node_id  TEXT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
  storage  TEXT NOT NULL CHECK (storage IN ('file','inline')),
  body     TEXT NOT NULL DEFAULT ''    -- inline only; file-backed notes live on disk
);
-- A board text card is a note with storage='inline' (G4). No is_board_text flag.

-- ----------------------------------------------------------- attributes

-- Key/value so a new attribute needs no migration (§1.4). value_num exists so
-- sorting and range queries never parse text.
CREATE TABLE IF NOT EXISTS attribute (
  node_id   TEXT NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  key       TEXT NOT NULL,
  value     TEXT,
  value_num REAL,
  PRIMARY KEY (node_id, key)
);
CREATE INDEX IF NOT EXISTS idx_attr_key     ON attribute(key, value_num);
CREATE INDEX IF NOT EXISTS idx_attr_key_txt ON attribute(key, value);

-- ---------------------------------------------------------------- edges

-- One table, one mechanism (§1.7). Compass is DERIVED from kind and is not
-- stored: v1 wrote direction='N' for both tagging and collector membership,
-- which made them distinguishable only by joining the target's type.
CREATE TABLE IF NOT EXISTS edge (
  id                 TEXT PRIMARY KEY,
  source_id          TEXT NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  target_id          TEXT REFERENCES node(id) ON DELETE CASCADE,  -- NULL only for unresolved wikilinks
  kind               TEXT NOT NULL CHECK (kind IN
                       ('contains','tag_of','compass_n','compass_s','compass_e','compass_w',
                        'wikilink','embed','board_position')),
  raw_target         TEXT,                    -- wikilink/embed: the literal [[text]] as written
  label              TEXT,
  ordinal            INTEGER NOT NULL DEFAULT 0,
  scope_collector_id TEXT REFERENCES node(id) ON DELETE CASCADE,
  status             TEXT NOT NULL DEFAULT 'declared' CHECK (status IN ('declared','suggested')),
  origin             TEXT NOT NULL DEFAULT 'user'
                       CHECK (origin IN ('user','cooccurrence','metadata','extension')),
  payload            TEXT,                    -- board_position: {"x":..,"y":..,"w":..,"h":..,"z":..}
  created_at         TEXT NOT NULL DEFAULT (datetime('now')),

  -- Only wikilinks and embeds may dangle.
  CHECK (target_id IS NOT NULL OR kind IN ('wikilink','embed')),
  CHECK (kind NOT IN ('wikilink','embed') OR raw_target IS NOT NULL)
);

-- Two partial indexes, because scope_collector_id is nullable (see header).
CREATE UNIQUE INDEX IF NOT EXISTS idx_edge_unique_global
  ON edge(source_id, target_id, kind)
  WHERE scope_collector_id IS NULL AND target_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_edge_unique_scoped
  ON edge(source_id, target_id, kind, scope_collector_id)
  WHERE scope_collector_id IS NOT NULL AND target_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_edge_source  ON edge(source_id, kind, ordinal);
CREATE INDEX IF NOT EXISTS idx_edge_target  ON edge(target_id, kind) WHERE target_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_edge_scope   ON edge(scope_collector_id) WHERE scope_collector_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_edge_status  ON edge(status) WHERE status = 'suggested';
CREATE INDEX IF NOT EXISTS idx_edge_dangling ON edge(raw_target) WHERE target_id IS NULL;

-- --------------------------------------------------- link resolution

-- Wikilinks resolve by title today, so renaming silently breaks every link
-- into a note (G9). This is the index that fixes it.
CREATE TABLE IF NOT EXISTS link_target (
  normalised TEXT NOT NULL,     -- casefolded, whitespace collapsed, diacritics stripped
  node_id    TEXT NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('title','alias','filename')),
  PRIMARY KEY (normalised, node_id, kind)
);
-- Resolution precedence is title > alias > filename, then oldest id. A tie is
-- reported as ambiguous, never silently resolved to one side.
CREATE INDEX IF NOT EXISTS idx_link_target ON link_target(normalised, kind);

-- ------------------------------------------------------ view prefs

-- Finder parity: each folder remembers how you last looked at it. User data,
-- so it belongs here and not in the UI store (G13).
CREATE TABLE IF NOT EXISTS view_prefs (
  scope_id   TEXT NOT NULL,               -- collector id, or a well-known scope name
  pane_kind  TEXT NOT NULL,
  layout     TEXT,
  sort       TEXT,
  group_by   TEXT,
  density    TEXT,
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (scope_id, pane_kind)
);

-- ------------------------------------------------ reconciliation log

-- Apply AND record (§9.1). The ladder is ordered and total, so the path is
-- implied by the destination: rule 9 firing means every applicable rule above
-- it was tested and declined. Storing the path as well would be a second copy
-- of a known fact, and two copies eventually disagree.
--
-- Rule 6 ("unchanged") is never written. It is most of every scan and would
-- bury the rows that matter; an idle scan writes nothing at all.
CREATE TABLE IF NOT EXISTS reconcile_log (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  at            TEXT NOT NULL DEFAULT (datetime('now')),
  node_id       TEXT,                      -- NULL when the rule refused to create a node
  table_version INTEGER NOT NULL,          -- which ladder was in force; makes old rows readable
  signals       INTEGER NOT NULL,          -- the eight observations packed into one byte
  rule          INTEGER NOT NULL,          -- derivable, stored because it is what queries filter on
  locator       TEXT
);
CREATE INDEX IF NOT EXISTS idx_log_at   ON reconcile_log(at);
CREATE INDEX IF NOT EXISTS idx_log_node ON reconcile_log(node_id) WHERE node_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_log_rule ON reconcile_log(rule);

-- ------------------------------------------------------------- search

-- One projection covers name, body and tag-association hits (G18), so the
-- index carries all three columns rather than the palette making two calls.
CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(
  node_id UNINDEXED,
  title,
  body,
  tags,
  tokenize = 'unicode61 remove_diacritics 2'
);

-- Kept in step by trigger, so there is one writer and no chance of the index
-- drifting from the nodes (invariant 10).
CREATE TRIGGER IF NOT EXISTS trg_search_ins AFTER INSERT ON node BEGIN
  INSERT INTO search(node_id, title, body, tags)
  VALUES (new.id, new.display_name, '', '');
END;

CREATE TRIGGER IF NOT EXISTS trg_search_upd AFTER UPDATE OF display_name ON node BEGIN
  UPDATE search SET title = new.display_name WHERE node_id = new.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_search_del AFTER DELETE ON node BEGIN
  DELETE FROM search WHERE node_id = old.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_search_body AFTER UPDATE OF body ON note BEGIN
  UPDATE search SET body = new.body WHERE node_id = new.node_id;
END;

-- ------------------------------------------------- discovery dismissals

-- Replaces the b_id DEFAULT 0 magic value with an explicit key (G19).
CREATE TABLE IF NOT EXISTS dismissed (
  dismiss_key TEXT PRIMARY KEY,
  kind        TEXT NOT NULL,
  at          TEXT NOT NULL DEFAULT (datetime('now'))
);
