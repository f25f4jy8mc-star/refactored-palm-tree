-- Staged inclusion, and settings that belong to a space.
--
-- `pending_enabled` is what the tickbox says; `enabled` is what is in effect.
-- Unticking a folder stages a change and nothing moves until Refresh applies
-- it — which is what makes Refresh the moment the library changes, rather
-- than content vanishing under the pointer as you click through a list.
-- NULL means "no pending change", so every existing row is already correct.
ALTER TABLE source ADD COLUMN pending_enabled INTEGER;

-- One space's own preferences: not a view's layout (that is `view_prefs`,
-- keyed by scope and pane) but a decision about the whole library, like
-- whether the folders you linked are drawn as items in it.
--
-- Key/value so a new setting needs no migration, the same reasoning as
-- `attribute` in §1.4. Values are text; a caller that wants a boolean says
-- so at the edge rather than the schema growing a column per question.
CREATE TABLE IF NOT EXISTS setting (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
