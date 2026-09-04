//! `p_record` — everything the model knows about one item, in one call.
//!
//! **Why a tenth projection.** The rule is one view, one projection, so that
//! two views can never compute the same thing differently. `p_detail` serves
//! the Inspector's links and measurements and is delivered, tested and not
//! being rewritten. The item record — identity, source, proxies, the facet
//! grid, the health parts and the indexing history — is more than p_detail
//! carries, and the wrong fix is to have the Inspector make five calls and
//! assemble the answer itself. That is precisely the "two components deciding
//! the same fact" shape the rebuild exists to remove.
//!
//! So this projection **wraps** p_detail rather than replacing or duplicating
//! it: `detail()` is called here, unchanged, and the extra blocks are read
//! alongside it. One view still reads exactly one projection.
//!
//! Nothing in this file computes anything a stored column already answers.
//! The health parts are read, not recalculated — `health::recompute` is the
//! single writer, and a projection that recomputed them could disagree with
//! what a list is sorting on.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::facets;
use super::health;
use super::projections::{self, Detail};
use super::suggest::{self, MetadataSuggestion};
use super::tags::{self, Tag};

/* --------------------------------------------------------- the ladder */

/// The thirteen rules, named. `reconcile.rs` owns the logic and is delivered
/// code; this is the reader's copy of what each outcome means, so a history
/// row can say "moved on the same volume" instead of "rule 10".
///
/// Tested against the ladder itself below: every rule the resolver can return
/// has an entry here, so adding a rule without naming it fails.
pub const RULES: &[(u8, &str, &str)] = &[
    (1, "Still being written", "Left alone until it settles — hashing a half-written file would churn every scan."),
    (2, "Could not be read", "The file is there and the system would not open it. Not the same as missing."),
    (3, "Duplicated note", "Two files claim one id and the original is still in place, so this one was given a fresh id."),
    (4, "Found by its id", "The id in the note's frontmatter settled it, whatever the path says."),
    (5, "Restored from elsewhere", "Carried an id we had never seen, and kept it — minting a new one would break every link inside the restored set."),
    (6, "Unchanged", "Seen and left alone. Never written to the log; most of every scan."),
    (7, "Edited in place", "Same file, new contents. The case that produced duplicates when the hash was the identity."),
    (8, "Replaced atomically", "An application wrote a temp file and renamed it over this one."),
    (9, "Replaced with new contents", "Either an atomic save or a different file dropped at this path — genuinely indistinguishable, and treated as the same item."),
    (10, "Moved on this volume", "Matched on device and inode after the path changed."),
    (11, "Moved from another volume", "Inode numbers do not cross drives, so this was matched on contents."),
    (12, "Copied", "Byte-identical to something already indexed, which still exists. Recorded as a pair rather than merged."),
    (13, "New", "Nothing else matched. A rise in this rule's rate is the sign of a bug further up the ladder."),
];

/// Bit order is fixed by `reconcile::Observation::packed` and must match it
/// exactly. The test at the bottom of this file packs real observations and
/// decodes them back, so the two cannot drift.
pub const SIGNALS: &[&str] = &[
    "in flight",
    "readable",
    "carries an id",
    "id is known",
    "original still elsewhere",
    "inode matched",
    "hash matched",
    "path matched",
];

pub fn decode_signals(packed: i64) -> Vec<String> {
    SIGNALS
        .iter()
        .enumerate()
        .filter(|(i, _)| packed & (1 << i) != 0)
        .map(|(_, name)| (*name).to_string())
        .collect()
}

/* ---------------------------------------------------------- the record */

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub node_type: String,
    pub content_type: String,
    /// The materialised conformance closure, leaf first. This is what makes a
    /// filter for "images" pick up HEIC without anyone maintaining a list.
    pub conforms_to: Vec<String>,
    pub title: String,
    pub display_name: String,
    pub display_subtitle: String,
    pub icon_kind: String,
    pub created_at: String,
    pub indexed_at: String,
    pub modified_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFacts {
    pub source_kind: String,
    pub locator: Option<String>,
    pub parent_dir: Option<String>,
    pub filename: Option<String>,
    pub extension: Option<String>,
    pub size_bytes: Option<i64>,
    /// BLAKE3, and an attribute rather than the identity. Nullable, and its
    /// index is deliberately not unique — two files may hold the same bytes.
    pub content_hash: Option<String>,
    pub inode: Option<i64>,
    pub device: Option<i64>,
    pub mtime: Option<String>,
    pub ctime: Option<String>,
    pub availability: String,
    pub last_seen_at: Option<String>,
}

/// Four artefacts, tracked separately (G3). Build 17 had one field for all of
/// them, which is why its "has a thumbnail" filter was really "has any proxy
/// at all".
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySet {
    pub thumb_ref: Option<String>,
    pub preview_ref: Option<String>,
    pub playable_ref: Option<String>,
    /// The original is not a proxy, but it is the fourth artefact, and a view
    /// deciding what to draw needs to know whether it can be reached.
    pub original_available: bool,
    pub version: i64,
    pub state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetSlot {
    pub facet: String,
    pub label: String,
    pub hint: String,
    pub tier: i64,
    pub machine_fillable: bool,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierBlock {
    pub tier: i64,
    pub label: String,
    pub facets: Vec<FacetSlot>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    /// Every facet, filled or not — an empty slot is the prompt, so leaving it
    /// out would hide the thing the view most needs to show.
    pub tiers: Vec<TierBlock>,
    pub suggestions: Vec<MetadataSuggestion>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthBlock {
    pub score: i64,
    pub label: String,
    pub description: String,
    pub facets_filled: i64,
    pub facet_target: i64,
    pub title_quality: i64,
    pub has_any_tag: i64,
    pub unresolved_links: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub at: String,
    pub rule: i64,
    pub rule_label: String,
    pub rule_note: String,
    pub signals: Vec<String>,
    pub table_version: i64,
    pub locator: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// p_detail, unchanged and unwrapped, so anything already reading a
    /// `Detail` shape keeps working.
    #[serde(flatten)]
    pub detail: Detail,
    pub identity: Identity,
    pub source: SourceFacts,
    pub proxies: ProxySet,
    pub classification: Classification,
    pub health: HealthBlock,
    /// What the reconciler decided about this item, newest first. Rule 6 is
    /// never written, so an item that has only ever been seen unchanged shows
    /// the pass that created it and nothing since.
    pub history: Vec<Event>,
}

pub fn record(conn: &Connection, id: &str) -> Result<Record> {
    let detail = projections::detail(conn, id, &projections::Options::default())?;

    let identity = conn.query_row(
        "SELECT id, node_type, content_type, content_type_tree, title, display_name,
                display_subtitle, icon_kind, created_at, indexed_at, modified_at
           FROM node WHERE id = ?1",
        params![id],
        |r| {
            let tree: String = r.get(3)?;
            Ok(Identity {
                id: r.get(0)?,
                node_type: r.get(1)?,
                content_type: r.get(2)?,
                conforms_to: serde_json::from_str(&tree).unwrap_or_default(),
                title: r.get(4)?,
                display_name: r.get(5)?,
                display_subtitle: r.get(6)?,
                icon_kind: r.get(7)?,
                created_at: r.get(8)?,
                indexed_at: r.get(9)?,
                modified_at: r.get(10)?,
            })
        },
    )?;

    let source = conn.query_row(
        "SELECT source_kind, locator, parent_dir, filename, extension, size_bytes,
                content_hash, inode, device, mtime, ctime, availability, last_seen_at
           FROM node WHERE id = ?1",
        params![id],
        |r| {
            Ok(SourceFacts {
                source_kind: r.get(0)?,
                locator: r.get(1)?,
                parent_dir: r.get(2)?,
                filename: r.get(3)?,
                extension: r.get(4)?,
                size_bytes: r.get(5)?,
                content_hash: r.get(6)?,
                inode: r.get(7)?,
                device: r.get(8)?,
                mtime: r.get(9)?,
                ctime: r.get(10)?,
                availability: r.get(11)?,
                last_seen_at: r.get(12)?,
            })
        },
    )?;

    let proxies = conn.query_row(
        "SELECT proxy_thumb_ref, proxy_preview_ref, proxy_playable_ref,
                proxy_version, proxy_state, availability
           FROM node WHERE id = ?1",
        params![id],
        |r| {
            let availability: String = r.get(5)?;
            Ok(ProxySet {
                thumb_ref: r.get(0)?,
                preview_ref: r.get(1)?,
                playable_ref: r.get(2)?,
                original_available: availability == "present",
                version: r.get(3)?,
                state: r.get(4)?,
            })
        },
    )?;

    let health_row = conn.query_row(
        "SELECT tagging_health, facets_filled, title_quality, has_any_tag, unresolved_links
           FROM node WHERE id = ?1",
        params![id],
        |r| {
            Ok(health::Components {
                score: r.get(0)?,
                facets_filled: r.get(1)?,
                title_quality: r.get(2)?,
                has_any_tag: r.get(3)?,
                unresolved_links: r.get(4)?,
            })
        },
    )?;
    let description = health::BUCKETS
        .iter()
        .find(|(n, _, _)| *n == health_row.score)
        .map(|(_, _, d)| (*d).to_string())
        .unwrap_or_default();

    Ok(Record {
        identity,
        source,
        proxies,
        classification: classification(conn, id)?,
        health: HealthBlock {
            score: health_row.score,
            label: health_row.label().to_string(),
            description,
            facets_filled: health_row.facets_filled,
            facet_target: health::FACET_TARGET,
            title_quality: health_row.title_quality,
            has_any_tag: health_row.has_any_tag,
            unresolved_links: health_row.unresolved_links,
        },
        history: history(conn, id)?,
        detail,
    })
}

/// The facet grid: every facet in tier order, each carrying the tags this item
/// holds in it. Empty slots are kept because an empty slot is the prompt.
fn classification(conn: &Connection, id: &str) -> Result<Classification> {
    let held = tags::of_node(conn, id)?;

    let mut tiers: Vec<TierBlock> = Vec::new();
    for f in facets::FACETS {
        // The holding pen only appears once something is in it — offering
        // "Unclassified" as a slot to fill would be an invitation to file
        // things nowhere.
        let mine: Vec<Tag> = held.iter().filter(|t| t.facet == f.id).cloned().collect();
        if f.tier == 0 && mine.is_empty() {
            continue;
        }
        let slot = FacetSlot {
            facet: f.id.to_string(),
            label: f.label.to_string(),
            hint: f.hint.to_string(),
            tier: f.tier,
            machine_fillable: f.machine_fillable,
            tags: mine,
        };
        match tiers.iter_mut().find(|t| t.tier == f.tier) {
            Some(block) => block.facets.push(slot),
            None => tiers.push(TierBlock {
                tier: f.tier,
                label: f.tier_label.to_string(),
                facets: vec![slot],
            }),
        }
    }
    tiers.sort_by_key(|t| if t.tier == 0 { i64::MAX } else { t.tier });

    Ok(Classification {
        tiers,
        suggestions: suggest::for_node(conn, id)?,
    })
}

fn history(conn: &Connection, id: &str) -> Result<Vec<Event>> {
    let mut q = conn.prepare(
        "SELECT at, rule, signals, table_version, locator
           FROM reconcile_log WHERE node_id = ?1 ORDER BY at DESC, id DESC LIMIT 50",
    )?;
    let out = q
        .query_map(params![id], |r| {
            let rule: i64 = r.get(1)?;
            let named = RULES.iter().find(|(n, _, _)| i64::from(*n) == rule);
            Ok(Event {
                at: r.get(0)?,
                rule,
                rule_label: named.map(|(_, l, _)| (*l).to_string()).unwrap_or_default(),
                rule_note: named.map(|(_, _, d)| (*d).to_string()).unwrap_or_default(),
                signals: decode_signals(r.get(2)?),
                table_version: r.get(3)?,
                locator: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::reconcile::{self, Observation};

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    fn item(c: &Connection, id: &str) {
        c.execute(
            "INSERT INTO node(id, node_type, content_type, content_type_tree,
                              display_name, filename, locator, size_bytes, content_hash,
                              proxy_thumb_ref, proxy_state, proxy_version, mtime)
             VALUES (?1,'media','public.jpeg','[\"public.jpeg\",\"public.image\",\"public.data\"]',
                     'Harbour wall','IMG_4821.jpg',?1, 1024, 'abc123',
                     '/proxies/abc123.jpg','ready',1,'2024-06-11T09:00:00Z')",
            params![id],
        )
        .unwrap();
    }

    #[test]
    fn every_rule_the_ladder_can_return_has_a_name() {
        // Sweeps all 256 signal combinations through the real resolver, for
        // notes and for media, and demands a label for every rule that comes
        // back. Adding a rule without naming it fails here.
        for bits in 0..256u16 {
            for is_note in [false, true] {
                let o = Observation {
                    flight: bits & 1 != 0,
                    readable: bits & 2 != 0,
                    id_present: bits & 4 != 0,
                    id_hit: bits & 8 != 0,
                    elsewhere: bits & 16 != 0,
                    inode: bits & 32 != 0,
                    hash: bits & 64 != 0,
                    path: bits & 128 != 0,
                    node_by_id: Some("x".into()),
                    node_by_inode: Some("x".into()),
                    node_by_hash: Some("x".into()),
                    node_by_path: Some("x".into()),
                    declared_id: Some("x".into()),
                };
                let rule = reconcile::resolve(&o, is_note).rule;
                assert!(
                    RULES.iter().any(|(n, _, _)| *n == rule),
                    "rule {rule} has no entry in RULES"
                );
            }
        }
    }

    #[test]
    fn the_signal_names_are_in_the_order_the_log_packs_them() {
        // Each signal alone, so a reordered bit shows up as the wrong name
        // rather than as a plausible-looking history row.
        let mut o = Observation::default();
        o.flight = true;
        assert_eq!(decode_signals(o.packed()), vec!["in flight"]);

        let mut o = Observation::default();
        o.path = true;
        assert_eq!(decode_signals(o.packed()), vec!["path matched"]);

        let mut o = Observation::default();
        o.inode = true;
        o.hash = true;
        assert_eq!(
            decode_signals(o.packed()),
            vec!["inode matched", "hash matched"]
        );
        assert_eq!(SIGNALS.len(), 8);
    }

    #[test]
    fn the_record_carries_the_whole_conformance_closure() {
        let c = db();
        item(&c, "a");
        let r = record(&c, "a").unwrap();
        assert_eq!(
            r.identity.conforms_to,
            vec!["public.jpeg", "public.image", "public.data"]
        );
        assert_eq!(r.identity.node_type, "media");
    }

    #[test]
    fn the_four_proxy_artefacts_are_reported_separately() {
        let c = db();
        item(&c, "a");
        let r = record(&c, "a").unwrap();
        assert_eq!(r.proxies.thumb_ref.as_deref(), Some("/proxies/abc123.jpg"));
        assert_eq!(r.proxies.preview_ref, None, "not the same field as the thumb");
        assert_eq!(r.proxies.playable_ref, None);
        assert!(r.proxies.original_available);
        assert_eq!(r.proxies.state, "ready");
    }

    #[test]
    fn the_hash_is_reported_as_an_attribute_beside_the_identity_not_as_it() {
        let c = db();
        item(&c, "a");
        let r = record(&c, "a").unwrap();
        assert_eq!(r.source.content_hash.as_deref(), Some("abc123"));
        assert_ne!(r.identity.id, "abc123");
    }

    #[test]
    fn every_facet_is_offered_even_when_empty() {
        let c = db();
        item(&c, "a");
        let r = record(&c, "a").unwrap();
        let named: Vec<&str> = r
            .classification
            .tiers
            .iter()
            .flat_map(|t| t.facets.iter())
            .map(|f| f.facet.as_str())
            .collect();
        assert_eq!(
            named,
            vec!["format", "era", "environment", "action", "attribute", "subject"],
            "an empty slot is the prompt"
        );
        assert_eq!(r.classification.tiers.len(), 3);
        assert_eq!(r.classification.tiers[0].label, "Metadata");
    }

    #[test]
    fn a_tag_appears_in_its_own_facet_and_nowhere_else() {
        let c = db();
        item(&c, "a");
        let t = tags::ensure(&c, "coast", "environment").unwrap();
        tags::apply(&c, &["a".to_string()], &t).unwrap();
        let r = record(&c, "a").unwrap();
        for tier in &r.classification.tiers {
            for f in &tier.facets {
                let expected = usize::from(f.facet == "environment");
                assert_eq!(f.tags.len(), expected, "{}", f.facet);
            }
        }
    }

    #[test]
    fn the_holding_pen_appears_only_once_something_is_in_it() {
        let c = db();
        item(&c, "a");
        let has_unfiled = |c: &Connection| {
            record(c, "a")
                .unwrap()
                .classification
                .tiers
                .iter()
                .any(|t| t.facets.iter().any(|f| f.facet == "unclassified"))
        };
        assert!(!has_unfiled(&c));
        let t = tags::ensure(&c, "todo", "unclassified").unwrap();
        tags::apply(&c, &["a".to_string()], &t).unwrap();
        assert!(has_unfiled(&c));
    }

    #[test]
    fn the_record_reads_stored_health_rather_than_recomputing_it() {
        // If this projection recalculated, it could disagree with what a list
        // is sorting on. Storing a deliberately wrong value proves it reads.
        let c = db();
        item(&c, "a");
        c.execute(
            "UPDATE node SET tagging_health = 3, facets_filled = 3, title_quality = 1,
                             has_any_tag = 1 WHERE id = 'a'",
            [],
        )
        .unwrap();
        let r = record(&c, "a").unwrap();
        assert_eq!(r.health.score, 3);
        assert_eq!(r.health.label, "Described");
        assert_eq!(r.health.facet_target, health::FACET_TARGET);
        assert!(!r.health.description.is_empty());
    }

    #[test]
    fn the_history_reads_back_named_and_newest_first() {
        let c = db();
        item(&c, "a");
        let mut o = Observation::default();
        o.readable = true;
        for (at, rule) in [("2024-01-01T00:00:00Z", 13), ("2024-02-01T00:00:00Z", 10)] {
            c.execute(
                "INSERT INTO reconcile_log(at, node_id, table_version, signals, rule, locator)
                 VALUES (?1, 'a', 1, ?2, ?3, '/x')",
                params![at, o.packed(), rule],
            )
            .unwrap();
        }
        let h = record(&c, "a").unwrap().history;
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].rule, 10);
        assert_eq!(h[0].rule_label, "Moved on this volume");
        assert_eq!(h[1].rule_label, "New");
        assert_eq!(h[0].signals, vec!["readable"]);
    }

    #[test]
    fn the_record_still_contains_everything_p_detail_returned() {
        let c = db();
        item(&c, "a");
        let r = record(&c, "a").unwrap();
        let d = projections::detail(&c, "a", &projections::Options::default()).unwrap();
        assert_eq!(r.detail.node.id, d.node.id);
        assert_eq!(r.detail.node.capabilities, d.node.capabilities);
        assert_eq!(r.detail.slots.len(), d.slots.len());
    }
}
