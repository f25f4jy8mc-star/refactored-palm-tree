-- How the Library is being read: `source` (every item once, folders left out)
-- or `hierarchy` (the tree). It belongs beside layout and sort because it is
-- the same kind of fact — how you last looked at this scope — and the same
-- argument applies: it survives a restart, so it is your data and not the
-- window's (G13).
--
-- Additive. `001_model.sql` is delivered and tested, and is not edited.
ALTER TABLE view_prefs ADD COLUMN shape TEXT;
