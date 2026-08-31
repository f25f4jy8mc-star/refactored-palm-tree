//! Deciding what happened to a file.
//!
//! Thirteen rules, read in order, first match wins. The order *is* the
//! algorithm: a rule only ever sees files the rules above it declined, so
//! reordering this function changes behaviour even though no condition
//! changes. Every rule exists because getting it backwards produces a
//! specific, nameable bug — those are in the comments.
//!
//! Nothing here touches the filesystem or the database. Signals go in, a
//! decision comes out, and it is tested by enumerating every combination.
//!
//! See archiva-reconciliation.html for the same table with worked examples.

use serde::Serialize;

/// Bump on any change to the rules — adding, removing or reordering. Stored
/// on every log row, because without it a recorded rule number means one thing
/// today and something else after the table changes.
pub const TABLE_VERSION: i64 = 1;

/// What the scanner observed. Eight booleans, and the identity of whatever
/// each lookup matched.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    /// Zero bytes, or modified inside the quiet window: still being written.
    pub flight: bool,
    /// We can open it. False means permissions, not absence.
    pub readable: bool,
    /// Frontmatter carries an `archiva-id`. Notes only; media cannot.
    pub id_present: bool,
    /// That id matches a node we already have.
    pub id_hit: bool,
    /// The matched node's recorded path still holds a file, and it is not this
    /// path. This one boolean is the whole difference between a move and a copy.
    pub elsewhere: bool,
    /// `(device, inode)` matches a known node.
    pub inode: bool,
    /// Content hash matches a known node.
    pub hash: bool,
    /// This exact path is already recorded.
    pub path: bool,

    // Which node each lookup found. Only consulted for the rule that fires.
    pub node_by_id: Option<String>,
    pub node_by_inode: Option<String>,
    pub node_by_hash: Option<String>,
    pub node_by_path: Option<String>,
    /// The id written in the file's frontmatter, whether or not we know it.
    pub declared_id: Option<String>,
}

impl Observation {
    /// The eight signals packed into one byte for the log. Bit order is fixed
    /// and must not be rearranged — `TABLE_VERSION` is what makes old rows
    /// readable, and it cannot rescue a silently reordered byte.
    pub fn packed(&self) -> i64 {
        let bits = [
            self.flight,
            self.readable,
            self.id_present,
            self.id_hit,
            self.elsewhere,
            self.inode,
            self.hash,
            self.path,
        ];
        bits.iter()
            .enumerate()
            .fold(0i64, |acc, (i, b)| if *b { acc | (1 << i) } else { acc })
    }
}

/// Where a new node's id comes from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum IdSource {
    /// Fresh UUID.
    Mint,
    /// Fresh UUID, and rewrite the file's frontmatter to match. Used when a
    /// note's stated id is already spoken for.
    MintAndRewrite,
    /// Keep the id the file already carries.
    Adopt(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Action {
    /// Change nothing at all. Not availability, not the hash, not last-seen.
    Defer,
    /// Mark unreadable. Keep everything already known about it.
    Unreadable { node_id: Option<String> },
    /// Nothing changed. Record that we saw it and stop.
    Touch { node_id: String },
    /// Same node, new facts. Path, inode, hash and mtime are all fair game.
    Update { node_id: String },
    /// A node we have not seen before.
    Create {
        id_source: IdSource,
        /// The node this is byte-identical to, when we can tell. Recorded so
        /// the pair can be reviewed rather than silently merged.
        copy_of: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    pub rule: u8,
    pub action: Action,
    /// True for the two readings the filesystem genuinely cannot settle.
    /// Worth surfacing rather than pretending to know.
    pub ambiguous: bool,
}

impl Resolution {
    fn new(rule: u8, action: Action) -> Self {
        Self {
            rule,
            action,
            ambiguous: matches!(rule, 9 | 12),
        }
    }

    /// Rule 6 is the overwhelming majority of every scan and says nothing
    /// happened. Logging it would bury the rows that matter, so an idle scan
    /// writes nothing at all.
    pub fn should_log(&self) -> bool {
        self.rule != 6
    }
}

/// The ladder.
pub fn resolve(o: &Observation, is_note: bool) -> Resolution {
    // Media carry no frontmatter, so the id signals are unavailable to them
    // rather than false. Forcing them here keeps every caller from having to
    // remember.
    let id_present = o.id_present && is_note;
    let id_hit = o.id_hit && is_note;

    // 1 — Still being written.
    // Hashing a half-written file records a fingerprint that will never match
    // again, so the next pass reads it as an edit, and the one after that as
    // another edit. Every pass churns.
    if o.flight {
        return Resolution::new(1, Action::Defer);
    }

    // 2 — Cannot be read.
    // Not the same as missing. Calling it missing tells the user their file is
    // gone when it is sitting right there, which is why availability is an
    // enum rather than a boolean.
    if !o.readable {
        return Resolution::new(
            2,
            Action::Unreadable {
                node_id: o.node_by_path.clone(),
            },
        );
    }

    // 3 — Duplicated note.
    // Two files claiming one id, and the original is still where it was.
    // Without this rule, which file wins depends on scan order.
    if id_present && id_hit && o.elsewhere {
        return Resolution::new(
            3,
            Action::Create {
                id_source: IdSource::MintAndRewrite,
                copy_of: o.node_by_id.clone(),
            },
        );
    }

    // 4 — Note found by its id.
    // The id settles it, which short-circuits every inode and path question
    // below. This is why the atomic-save problem is a media-only problem.
    if id_present && id_hit {
        return Resolution::new(
            4,
            Action::Update {
                node_id: o.node_by_id.clone().expect("id_hit implies a match"),
            },
        );
    }

    // 5 — Note from somewhere else.
    // Restored from backup after the index was lost, or copied in from another
    // library. Minting a new id would break every link inside the restored set,
    // since those notes reference each other by the old ids.
    if id_present && !id_hit {
        let declared = o.declared_id.clone().expect("id_present implies a value");
        return Resolution::new(
            5,
            Action::Create {
                id_source: IdSource::Adopt(declared),
                copy_of: None,
            },
        );
    }

    // 6 — Unchanged. The common case by a wide margin, so it must be cheap:
    // inode and mtime are checked before a hash is ever computed.
    if o.inode && o.path && o.hash {
        return Resolution::new(
            6,
            Action::Touch {
                node_id: o.node_by_path.clone().expect("path implies a match"),
            },
        );
    }

    // 7 — Edited in place.
    // The case Build 17 gets wrong by using the hash as identity: an edit
    // produces a new fingerprint, the node looks new, and every link is orphaned.
    if o.inode && o.path {
        return Resolution::new(
            7,
            Action::Update {
                node_id: o.node_by_path.clone().expect("path implies a match"),
            },
        );
    }

    // 8 — Replaced atomically.
    // An application wrote a temp file and renamed it over the original.
    // Inode-first matching calls this new; path-based matching calls it
    // existing. Two code paths, two answers, one duplicate.
    if o.path && o.hash {
        return Resolution::new(
            8,
            Action::Update {
                node_id: o.node_by_path.clone().expect("path implies a match"),
            },
        );
    }

    // 9 — Replaced with new contents. AMBIGUOUS.
    // Either an atomic save of an edit, or a different file dropped at this
    // path. Indistinguishable. Calling it a new node would orphan every tag on
    // a routine save, which is frequent and silent; calling it the same node is
    // wrong only when a file is deliberately swapped, which is rare and
    // visible. Pick the wrong answer that is cheap to notice.
    if o.path {
        return Resolution::new(
            9,
            Action::Update {
                node_id: o.node_by_path.clone().expect("path implies a match"),
            },
        );
    }

    // 10 — Moved on the same volume.
    // The match is on (device, inode), never the inode alone: inode numbers are
    // unique per device and would collide across drives.
    if o.inode {
        return Resolution::new(
            10,
            Action::Update {
                node_id: o.node_by_inode.clone().expect("inode implies a match"),
            },
        );
    }

    // 11 — Moved to another volume.
    // The inode is meaningless across the boundary, so contents are all we have.
    if o.hash && !o.elsewhere {
        return Resolution::new(
            11,
            Action::Update {
                node_id: o.node_by_hash.clone().expect("hash implies a match"),
            },
        );
    }

    // 12 — Copied. AMBIGUOUS.
    // Separated from a move by one thing: whether the original still exists.
    // Get that check wrong and every copy silently relocates the original,
    // taking its tags with it.
    if o.hash {
        return Resolution::new(
            12,
            Action::Create {
                id_source: IdSource::Mint,
                copy_of: o.node_by_hash.clone(),
            },
        );
    }

    // 13 — New.
    // The bottom of the ladder, so it fires only when every other reading has
    // been ruled out. A bug anywhere above shows up here as a spurious
    // duplicate, which makes this rule's rate the number to watch.
    Resolution::new(
        13,
        Action::Create {
            id_source: if is_note {
                IdSource::MintAndRewrite
            } else {
                IdSource::Mint
            },
            copy_of: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(bits: [bool; 8]) -> Observation {
        Observation {
            flight: bits[0],
            readable: bits[1],
            id_present: bits[2],
            id_hit: bits[3],
            elsewhere: bits[4],
            inode: bits[5],
            hash: bits[6],
            path: bits[7],
            node_by_id: Some("by-id".into()),
            node_by_inode: Some("by-inode".into()),
            node_by_hash: Some("by-hash".into()),
            node_by_path: Some("by-path".into()),
            declared_id: Some("declared".into()),
        }
    }

    /// f, r, idp, idh, els, ino, hash, path
    fn case(f: u8, r: u8, idp: u8, idh: u8, e: u8, i: u8, h: u8, p: u8) -> Observation {
        obs([f, r, idp, idh, e, i, h, p].map(|b| b == 1))
    }

    #[test]
    fn every_real_scenario_lands_where_it_should() {
        //                              f  r idp idh els ino  h  p    note   rule
        let cases: &[(&str, Observation, bool, u8)] = &[
            ("Obsidian saved a note", case(0, 1, 1, 1, 0, 0, 0, 1), true, 4),
            ("renamed a photo", case(0, 1, 0, 0, 0, 1, 1, 0), false, 10),
            ("folder to an external drive", case(0, 1, 0, 0, 0, 0, 1, 0), false, 11),
            ("duplicated a note file", case(0, 1, 1, 1, 1, 0, 1, 0), true, 3),
            ("copied a photo", case(0, 1, 0, 0, 1, 0, 1, 0), false, 12),
            ("Photoshop save", case(0, 1, 0, 0, 0, 0, 0, 1), false, 9),
            ("Time Machine restore", case(0, 1, 1, 0, 0, 0, 0, 0), true, 5),
            ("caught mid-download", case(1, 1, 0, 0, 0, 0, 0, 0), false, 1),
            ("locked folder", case(0, 0, 0, 0, 0, 0, 0, 0), false, 2),
            ("nothing changed", case(0, 1, 0, 0, 0, 1, 1, 1), false, 6),
            ("exiftool wrote in place", case(0, 1, 0, 0, 0, 1, 0, 1), false, 7),
            ("Dropbox re-synced", case(0, 1, 0, 0, 0, 0, 1, 1), false, 8),
            ("a new photo", case(0, 1, 0, 0, 0, 0, 0, 0), false, 13),
        ];
        for (label, o, is_note, want) in cases {
            let got = resolve(o, *is_note);
            assert_eq!(got.rule, *want, "{label} resolved to rule {}", got.rule);
        }
    }

    /// Invariant 3: every combination of signals must reach exactly one rule.
    /// A hole here is a file the indexer would silently do nothing about.
    #[test]
    fn the_ladder_is_total() {
        for is_note in [false, true] {
            for m in 0u16..256 {
                let bits = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| m & (1 << i) != 0);
                let r = resolve(&obs(bits), is_note);
                assert!((1..=13).contains(&r.rule), "rule {} out of range", r.rule);
            }
        }
    }

    /// Media have no frontmatter, so the notes-only rules must be unreachable
    /// for them however the id bits happen to be set.
    #[test]
    fn notes_only_rules_never_fire_for_media() {
        for m in 0u16..256 {
            let bits = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| m & (1 << i) != 0);
            let r = resolve(&obs(bits), false);
            assert!(!(3..=5).contains(&r.rule), "media reached rule {}", r.rule);
        }
    }

    /// The one boolean that separates a move from a copy. Everything else is
    /// held constant here on purpose.
    #[test]
    fn elsewhere_alone_decides_move_versus_copy() {
        let moved = resolve(&case(0, 1, 0, 0, 0, 0, 1, 0), false);
        let copied = resolve(&case(0, 1, 0, 0, 1, 0, 1, 0), false);
        assert_eq!(moved.rule, 11);
        assert!(matches!(moved.action, Action::Update { .. }));
        assert_eq!(copied.rule, 12);
        assert!(matches!(copied.action, Action::Create { .. }));
    }

    /// An edit must never produce a second node. This is the duplicate-notes
    /// bug, stated as a test.
    #[test]
    fn editing_never_creates_a_node() {
        let in_place = resolve(&case(0, 1, 0, 0, 0, 1, 0, 1), false);
        let atomic = resolve(&case(0, 1, 0, 0, 0, 0, 0, 1), false);
        let note_saved = resolve(&case(0, 1, 1, 1, 0, 0, 0, 1), true);
        for r in [in_place, atomic, note_saved] {
            assert!(
                matches!(r.action, Action::Update { .. }),
                "rule {} created a node on an edit",
                r.rule
            );
        }
    }

    #[test]
    fn only_two_rules_are_ambiguous() {
        let mut amb = Vec::new();
        for is_note in [false, true] {
            for m in 0u16..256 {
                let bits = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| m & (1 << i) != 0);
                let r = resolve(&obs(bits), is_note);
                if r.ambiguous && !amb.contains(&r.rule) {
                    amb.push(r.rule);
                }
            }
        }
        amb.sort();
        assert_eq!(amb, vec![9, 12]);
    }

    #[test]
    fn only_unchanged_is_unlogged() {
        for is_note in [false, true] {
            for m in 0u16..256 {
                let bits = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| m & (1 << i) != 0);
                let r = resolve(&obs(bits), is_note);
                assert_eq!(r.should_log(), r.rule != 6);
            }
        }
    }

    #[test]
    fn signal_packing_is_stable() {
        assert_eq!(case(0, 0, 0, 0, 0, 0, 0, 0).packed(), 0);
        assert_eq!(case(1, 0, 0, 0, 0, 0, 0, 0).packed(), 1);
        assert_eq!(case(0, 1, 0, 0, 0, 0, 0, 0).packed(), 2);
        assert_eq!(case(0, 0, 0, 0, 0, 0, 0, 1).packed(), 128);
        assert_eq!(case(1, 1, 1, 1, 1, 1, 1, 1).packed(), 255);
    }
}
