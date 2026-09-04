//! The facets, and the tiers they fall into.
//!
//! Checklist C1. Six facets in three tiers — plus `unclassified` at tier 0,
//! which the migration's CHECK already allows and which a tag needs somewhere
//! to sit while you decide what it is. Calling that a seventh facet would be
//! wrong: it is the absence of one, and it is why the tier column runs 0–3
//! rather than 1–3.
//!
//! This exists as executable data for the same reason `capabilities.rs` does.
//! The tier is stored next to the facet in the `tag` table even though the
//! facet determines it, because filtering by tier should be an index lookup
//! rather than a join — and a denormalised fact needs one place that owns it.
//! That place is here, and the SQL CHECK is the second opinion: the tests
//! below write every pairing this file believes in straight at the real table,
//! so the two cannot drift without a test going red.
//!
//! The three tiers are also the three layers the map cycles through:
//! structural, then contextual, then interpretive.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Facet {
    pub id: &'static str,
    pub label: &'static str,
    pub tier: i64,
    pub tier_label: &'static str,
    /// What kind of judgement this facet asks for. Shown next to the picker,
    /// because "Environment" on its own does not tell anyone what to type.
    pub hint: &'static str,
    /// Whether the indexer can propose values for this facet from file
    /// metadata alone (C5). Tier 1 can; the rest is human judgement.
    pub machine_fillable: bool,
}

/// Declaration order is display order: tier 1 first, then 2, then 3, with
/// `unclassified` last because it is a holding pen rather than a category.
pub const FACETS: &[Facet] = &[
    Facet {
        id: "format",
        label: "Format",
        tier: 1,
        tier_label: "Metadata",
        hint: "What it physically is — 35mm, screenshot, scan, render.",
        machine_fillable: true,
    },
    Facet {
        id: "era",
        label: "Era",
        tier: 1,
        tier_label: "Metadata",
        hint: "When it is from — a decade, a period, a project year.",
        machine_fillable: true,
    },
    Facet {
        id: "environment",
        label: "Environment",
        tier: 2,
        tier_label: "Classification",
        hint: "Where it is — interior, coastline, studio, street.",
        machine_fillable: false,
    },
    Facet {
        id: "action",
        label: "Action",
        tier: 2,
        tier_label: "Classification",
        hint: "What is happening — building, waiting, decaying.",
        machine_fillable: false,
    },
    Facet {
        id: "attribute",
        label: "Attribute",
        tier: 3,
        tier_label: "Content",
        hint: "How it reads — muted, symmetrical, cluttered.",
        machine_fillable: false,
    },
    Facet {
        id: "subject",
        label: "Subject",
        tier: 3,
        tier_label: "Content",
        hint: "What it is of — the person, place or thing pictured.",
        machine_fillable: false,
    },
    Facet {
        id: "unclassified",
        label: "Unclassified",
        tier: 0,
        tier_label: "Unfiled",
        hint: "Not yet placed. A tag can live here indefinitely.",
        machine_fillable: false,
    },
];

/// The six real facets — everything except the holding pen. This is the count
/// `facets_filled` is measured against, so "six facets in three tiers" stays
/// one statement rather than a number repeated in three files.
pub const CLASSIFYING: usize = 6;

pub fn get(id: &str) -> Option<&'static Facet> {
    FACETS.iter().find(|f| f.id == id)
}

/// The tier a facet belongs to, or `None` if the facet is not one we know.
/// Callers use this rather than passing a tier in: a caller that can choose
/// the tier is a caller that can get it wrong.
pub fn tier_of(facet: &str) -> Option<i64> {
    get(facet).map(|f| f.tier)
}

pub fn is_facet(id: &str) -> bool {
    get(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    fn insert(c: &Connection, id: &str, facet: &str, tier: i64) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO node(id, node_type, content_type, display_name)
             VALUES (?1, 'tag', 'app.archiva.virtual', ?1)",
            params![id],
        )?;
        c.execute(
            "INSERT INTO tag(node_id, facet, tier) VALUES (?1, ?2, ?3)",
            params![id, facet, tier],
        )
    }

    #[test]
    fn every_facet_this_file_declares_is_one_the_schema_accepts() {
        let c = db();
        for (i, f) in FACETS.iter().enumerate() {
            let id = format!("t{i}");
            insert(&c, &id, f.id, f.tier)
                .unwrap_or_else(|e| panic!("schema rejected {} at tier {}: {e}", f.id, f.tier));
        }
    }

    #[test]
    fn the_schema_rejects_a_facet_placed_in_the_wrong_tier() {
        // The denormalised column is only safe because this fails. If it ever
        // starts passing, tier has silently become a field a caller can lie in.
        let c = db();
        assert!(insert(&c, "t1", "subject", 1).is_err());
        assert!(insert(&c, "t2", "format", 3).is_err());
        assert!(insert(&c, "t3", "unclassified", 2).is_err());
    }

    #[test]
    fn the_schema_rejects_a_facet_this_file_does_not_declare() {
        let c = db();
        assert!(insert(&c, "t1", "mood", 3).is_err());
        assert!(!is_facet("mood"));
    }

    #[test]
    fn six_facets_classify_and_one_holds() {
        assert_eq!(FACETS.iter().filter(|f| f.tier > 0).count(), CLASSIFYING);
        assert_eq!(FACETS.iter().filter(|f| f.tier == 0).count(), 1);
    }

    #[test]
    fn the_three_tiers_hold_two_facets_each() {
        for tier in 1..=3 {
            assert_eq!(
                FACETS.iter().filter(|f| f.tier == tier).count(),
                2,
                "tier {tier}"
            );
        }
    }

    #[test]
    fn only_tier_one_is_fillable_from_metadata() {
        // C5's boundary: Format and Era can be proposed from what the file
        // already says about itself. Everything below tier 1 is judgement,
        // and proposing it would be the machine classifying rather than
        // suggesting.
        for f in FACETS {
            assert_eq!(f.machine_fillable, f.tier == 1, "{}", f.id);
        }
    }

    #[test]
    fn tier_lookup_refuses_what_it_does_not_know() {
        assert_eq!(tier_of("era"), Some(1));
        assert_eq!(tier_of("subject"), Some(3));
        assert_eq!(tier_of("unclassified"), Some(0));
        assert_eq!(tier_of("nonsense"), None);
    }
}
