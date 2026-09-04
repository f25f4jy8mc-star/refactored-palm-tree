//! How much of an item's description is filled in.
//!
//! Checklist C7, gap G20. Build 17 kept one integer and worked out what it
//! meant inside the view, with a switch statement per view — so two views
//! could and did disagree about what "2" was. Here the score and its parts are
//! computed in one place and stored, and the bucket labels live next to the
//! thresholds that produce them.
//!
//! **The parts are not decoration.** One integer cannot tell
//! well-tagged-but-badly-named from well-named-but-untagged, and those need
//! different prompts. `facets_filled`, `title_quality`, `has_any_tag` and
//! `unresolved_links` are what make the prompt specific.
//!
//! The scale is fixed by code that was delivered and is not being rewritten:
//! `projections::to_row` already reads `facets_filled < 3` and phrases it as
//! "N of 3 facets", and treats `title_quality == 0` as "filename as title".
//! So three filled facets is the target and `title_quality` is a flag, not a
//! range. Widening either would silently change what a tested projection says.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

/// What the 0–3 score means, with the threshold beside the label so a view
/// never has to restate it. This is the switch statement G20 complained
/// about, written once.
pub const BUCKETS: &[(i64, &str, &str)] = &[
    (0, "Untouched", "No tags at all."),
    (1, "Started", "One facet filled."),
    (2, "Partly described", "Two facets, or three without a title."),
    (3, "Described", "Three or more facets, and a title of its own."),
];

/// Facets filled before an item counts as fully described. Three of the six,
/// not all six — every facet is optional per item, and demanding all of them
/// is the paperwork that kills a library at item forty.
pub const FACET_TARGET: i64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Components {
    /// Distinct facets (of the six classifying ones) with at least one tag.
    pub facets_filled: i64,
    /// 0 when the name is still the one the filesystem gave it, 1 once it is
    /// the user's. Deliberately a flag — see the module note.
    pub title_quality: i64,
    pub has_any_tag: i64,
    /// Wikilinks and embeds written in this note that point at nothing yet.
    pub unresolved_links: i64,
    pub score: i64,
}

impl Components {
    pub fn label(&self) -> &'static str {
        BUCKETS
            .iter()
            .find(|(n, _, _)| *n == self.score)
            .map(|(_, l, _)| *l)
            .unwrap_or("Unknown")
    }
}

/// Score from the parts. Pure, so the ladder can be read without a database.
///
/// The title only ever *withholds* the top mark: an untitled item with five
/// facets is well classified and badly named, which is a different problem
/// from an untagged one and should not read as the same score.
pub fn score(facets_filled: i64, title_quality: i64) -> i64 {
    let s = facets_filled.min(FACET_TARGET);
    if title_quality == 0 {
        s.min(2)
    } else {
        s
    }
}

/// A name is the user's unless it is still the one the file arrived with.
///
/// Two ways it can fail: the display name is literally the filename stem, or
/// it matches one of the shapes cameras, phones and screenshot tools produce.
/// Those are not titles — they are serial numbers, and an archive full of
/// `IMG_4821` is the thing Scattered exists to surface.
pub fn title_is_the_users(display_name: &str, filename: Option<&str>) -> bool {
    let name = display_name.trim();
    if name.is_empty() {
        return false;
    }
    if let Some(file) = filename {
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        if stem.eq_ignore_ascii_case(name) {
            return false;
        }
    }
    !looks_machine_made(name)
}

fn looks_machine_made(name: &str) -> bool {
    let lower = name.to_lowercase();
    const PREFIXES: &[&str] = &[
        "img_", "img-", "dsc_", "dsc-", "dscf", "p10", "pxl_", "mvimg_", "vid_",
        "screenshot", "screen shot", "photo on ", "scan_", "scan-", "untitled",
    ];
    if PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return true;
    }
    // All digits, separators and nothing else: 2024-06-11 14.22.03, 00123.
    name.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | ' ' | ':'))
}

/// Read the parts for one node without writing them.
pub fn components(conn: &Connection, id: &str) -> Result<Components> {
    let (node_type, display_name, filename): (String, String, Option<String>) = conn.query_row(
        "SELECT node_type, display_name, filename FROM node WHERE id = ?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    // A collector or a tag is not a described item — it is the describing.
    // Scoring them zero would fill Scattered with folders, which is the
    // opposite of what that view is for.
    if node_type == "collector" || node_type == "tag" {
        return Ok(Components {
            facets_filled: 0,
            title_quality: 1,
            has_any_tag: 0,
            unresolved_links: 0,
            score: 3,
        });
    }

    let facets_filled: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT t.facet)
           FROM edge e JOIN tag t ON t.node_id = e.target_id
          WHERE e.source_id = ?1 AND e.kind = 'tag_of' AND t.facet <> 'unclassified'",
        params![id],
        |r| r.get(0),
    )?;
    let tag_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edge WHERE source_id = ?1 AND kind = 'tag_of'",
        params![id],
        |r| r.get(0),
    )?;
    let unresolved_links: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edge
          WHERE source_id = ?1 AND kind IN ('wikilink','embed') AND target_id IS NULL",
        params![id],
        |r| r.get(0),
    )?;

    let title_quality = i64::from(title_is_the_users(&display_name, filename.as_deref()));
    Ok(Components {
        facets_filled,
        title_quality,
        has_any_tag: i64::from(tag_count > 0),
        unresolved_links,
        score: score(facets_filled, title_quality),
    })
}

/// Recompute and store. Called after anything that changes a tag, a title or
/// a link — one writer, so the stored parts can never be stale in a way a view
/// would have to guess about.
pub fn recompute(conn: &Connection, id: &str) -> Result<Components> {
    let c = components(conn, id)?;
    conn.execute(
        "UPDATE node SET tagging_health = ?2, facets_filled = ?3, title_quality = ?4,
                         has_any_tag = ?5, unresolved_links = ?6
          WHERE id = ?1",
        params![
            id,
            c.score,
            c.facets_filled,
            c.title_quality,
            c.has_any_tag,
            c.unresolved_links
        ],
    )?;
    Ok(c)
}

pub fn recompute_many(conn: &Connection, ids: &[String]) -> Result<()> {
    for id in ids {
        recompute(conn, id)?;
    }
    Ok(())
}

/// Every item in the library. Used after a scan, and after a tag is merged or
/// deleted — both of which change the health of items this call has no other
/// way of naming.
pub fn recompute_all(conn: &Connection) -> Result<usize> {
    let mut q = conn.prepare("SELECT id FROM node")?;
    let ids: Vec<String> = q
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    drop(q);
    for id in &ids {
        recompute(conn, id)?;
    }
    Ok(ids.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tags;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    fn item(c: &Connection, id: &str, name: &str, filename: &str) {
        c.execute(
            "INSERT INTO node(id, node_type, content_type, display_name, filename, locator)
             VALUES (?1, 'media', 'public.jpeg', ?2, ?3, ?1)",
            params![id, name, filename],
        )
        .unwrap();
    }

    #[test]
    fn an_untouched_item_scores_zero_and_says_why() {
        let c = db();
        item(&c, "a", "IMG_4821", "IMG_4821.jpg");
        let h = recompute(&c, "a").unwrap();
        assert_eq!(h.score, 0);
        assert_eq!(h.has_any_tag, 0);
        assert_eq!(h.title_quality, 0);
        assert_eq!(h.label(), "Untouched");
    }

    #[test]
    fn the_score_climbs_one_facet_at_a_time() {
        let c = db();
        item(&c, "a", "Harbour wall", "IMG_4821.jpg");
        let ids = vec!["a".to_string()];
        for (i, (name, facet)) in [
            ("35mm", "format"),
            ("1970s", "era"),
            ("coast", "environment"),
        ]
        .iter()
        .enumerate()
        {
            let t = tags::ensure(&c, name, facet).unwrap();
            tags::apply(&c, &ids, &t).unwrap();
            assert_eq!(recompute(&c, "a").unwrap().score, i as i64 + 1);
        }
    }

    #[test]
    fn two_tags_in_one_facet_are_still_one_facet() {
        // Otherwise ten Subject tags would read as fully described.
        let c = db();
        item(&c, "a", "Harbour wall", "IMG_4821.jpg");
        let ids = vec!["a".to_string()];
        for name in ["boat", "rope", "gull"] {
            let t = tags::ensure(&c, name, "subject").unwrap();
            tags::apply(&c, &ids, &t).unwrap();
        }
        let h = recompute(&c, "a").unwrap();
        assert_eq!(h.facets_filled, 1);
        assert_eq!(h.score, 1);
        assert_eq!(h.has_any_tag, 1);
    }

    #[test]
    fn an_unfiled_tag_counts_as_a_tag_but_fills_no_facet() {
        let c = db();
        item(&c, "a", "Harbour wall", "IMG_4821.jpg");
        let t = tags::ensure(&c, "todo", "unclassified").unwrap();
        tags::apply(&c, &["a".to_string()], &t).unwrap();
        let h = recompute(&c, "a").unwrap();
        assert_eq!(h.has_any_tag, 1);
        assert_eq!(h.facets_filled, 0);
        assert_eq!(h.score, 0);
    }

    #[test]
    fn the_filename_as_a_title_withholds_the_top_mark_but_nothing_else() {
        let c = db();
        item(&c, "a", "IMG_4821", "IMG_4821.jpg");
        let ids = vec!["a".to_string()];
        for (name, facet) in [("35mm", "format"), ("1970s", "era"), ("coast", "environment")] {
            let t = tags::ensure(&c, name, facet).unwrap();
            tags::apply(&c, &ids, &t).unwrap();
        }
        let h = recompute(&c, "a").unwrap();
        assert_eq!(h.facets_filled, 3, "the classification is complete");
        assert_eq!(h.score, 2, "but the name is still the camera's");
        assert_eq!(h.label(), "Partly described");
    }

    #[test]
    fn a_camera_name_is_not_a_title_however_it_is_spelled() {
        for machine in [
            "IMG_4821",
            "dsc_0001",
            "Screenshot 2026-09-04 at 11.20.15",
            "PXL_20240611_142203",
            "2024-06-11 14.22.03",
            "untitled",
            "00123",
        ] {
            assert!(
                !title_is_the_users(machine, None),
                "{machine} should not count as a title"
            );
        }
        for human in ["Harbour wall", "Bergamo, day two", "A4 test print"] {
            assert!(title_is_the_users(human, None), "{human} should count");
        }
    }

    #[test]
    fn a_title_that_merely_repeats_the_filename_is_not_a_title() {
        assert!(!title_is_the_users("Harbour wall", Some("Harbour wall.jpg")));
        assert!(title_is_the_users("Harbour wall", Some("IMG_4821.jpg")));
    }

    #[test]
    fn collectors_and_tags_are_never_reported_as_unprocessed() {
        let c = db();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name)
             VALUES ('c','collector','app.archiva.virtual','Trips')",
            [],
        )
        .unwrap();
        let t = tags::ensure(&c, "coast", "environment").unwrap();
        assert_eq!(recompute(&c, "c").unwrap().score, 3);
        assert_eq!(recompute(&c, &t).unwrap().score, 3);
    }

    #[test]
    fn dangling_wikilinks_are_counted_and_resolved_ones_are_not() {
        let c = db();
        item(&c, "a", "Note", "a.md");
        item(&c, "b", "Other", "b.md");
        c.execute(
            "INSERT INTO edge(id,source_id,target_id,kind,raw_target)
             VALUES ('e1','a',NULL,'wikilink','[[nothing]]')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO edge(id,source_id,target_id,kind,raw_target)
             VALUES ('e2','a','b','wikilink','[[other]]')",
            [],
        )
        .unwrap();
        assert_eq!(recompute(&c, "a").unwrap().unresolved_links, 1);
    }

    #[test]
    fn the_stored_parts_match_what_was_computed() {
        let c = db();
        item(&c, "a", "Harbour wall", "IMG_4821.jpg");
        let t = tags::ensure(&c, "35mm", "format").unwrap();
        tags::apply(&c, &["a".to_string()], &t).unwrap();
        let computed = recompute(&c, "a").unwrap();
        let stored: (i64, i64, i64, i64) = c
            .query_row(
                "SELECT tagging_health, facets_filled, title_quality, has_any_tag
                   FROM node WHERE id = 'a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            stored,
            (
                computed.score,
                computed.facets_filled,
                computed.title_quality,
                computed.has_any_tag
            )
        );
    }

    #[test]
    fn every_score_the_ladder_can_produce_has_a_label() {
        for facets in 0..=6 {
            for title in 0..=1 {
                let s = score(facets, title);
                assert!(
                    BUCKETS.iter().any(|(n, _, _)| *n == s),
                    "no label for score {s}"
                );
            }
        }
    }
}
