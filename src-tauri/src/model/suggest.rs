//! Proposals. Never applications.
//!
//! Checklist C4 and C5, and principle 3: the machine suggests, the user
//! classifies. Nothing in this file writes a tag or an edge. It returns things
//! a person can accept, and remembers what they waved away.
//!
//! Two kinds, both tier 1 of the suggestion ladder — arithmetic on what is
//! already stored, with no machine learning anywhere:
//!
//!   * **near-duplicate tags** (C4): two tags in one facet that differ by a
//!     character or a plural. Offered as a merge.
//!   * **metadata suggestions** (C5): Format and Era read off what the file
//!     already says about itself. Tier 1 is the only tier that can be
//!     proposed this way — Environment, Action, Attribute and Subject are
//!     judgements, and a machine offering them is a machine classifying.
//!
//! Dismissal is permanent and explicit, keyed in the `dismissed` table (G19).
//! Dismissing a near-duplicate keeps both tags forever, which is the point:
//! "singer" and "singers" may well be two different claims, and being asked
//! again every week is how a suggestion becomes noise.
//!
//! **Dominant colour is deliberately not here.** The checklist lists it as a
//! proposed Attribute, and it was flagged as cheap to keep and easy to drop.
//! Reading the average colour of a photograph means reading its pixels, and
//! `extract` states as principle 4 that nothing measures *into* a file's
//! content. Implementing it would break that rule quietly in a second file.
//! It is one line of work once you decide the rule should bend; it should not
//! bend by accident.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::facets;
use super::tags::{self, Tag};

#[derive(Debug, Serialize)]
pub struct DuplicatePair {
    pub key: String,
    pub a: Tag,
    pub b: Tag,
    /// Why these two were paired, in the words a person would use.
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct MetadataSuggestion {
    pub key: String,
    pub facet: String,
    pub name: String,
    /// What in the file produced this. Shown, because an unexplained
    /// suggestion is indistinguishable from a guess.
    pub evidence: String,
}

/* ------------------------------------------------------- near duplicates */

/// Tags in the same facet that differ by one edit or a plural.
///
/// Same facet only. Two identical words in *different* facets are a
/// deliberate distinction — "coastline" the Environment and "coastline" the
/// Subject are separate claims, and `tags::ensure` is built to allow exactly
/// that. Offering to merge them here would fight the rule one file over.
pub fn near_duplicates(conn: &Connection) -> Result<Vec<DuplicatePair>> {
    let all = tags::list(conn)?;
    let mut out = Vec::new();
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            let (a, b) = (&all[i], &all[j]);
            if a.facet != b.facet {
                continue;
            }
            let (x, y) = (tags::normalise(&a.name), tags::normalise(&b.name));
            let Some(reason) = pair_reason(&x, &y) else {
                continue;
            };
            let key = duplicate_key(&a.id, &b.id);
            if is_dismissed(conn, &key)? {
                continue;
            }
            out.push(DuplicatePair {
                key,
                // The more-used tag leads, because it is the likelier merge
                // target and the view offers the pair in the order given.
                a: if a.usage >= b.usage { a.clone() } else { b.clone() },
                b: if a.usage >= b.usage { b.clone() } else { a.clone() },
                reason,
            });
        }
    }
    Ok(out)
}

fn pair_reason(a: &str, b: &str) -> Option<String> {
    if a == b {
        return Some("the same name".into());
    }
    if plural_of(a, b) || plural_of(b, a) {
        return Some("a plural of the other".into());
    }
    // Guard the cheap case first: a one-character edit cannot bridge a length
    // gap of more than one, and short words are too easily paired by accident
    // ("cat"/"car" are not a typo for each other).
    if a.len().abs_diff(b.len()) > 1 || a.len() < 4 || b.len() < 4 {
        return None;
    }
    if edit_distance(a, b) <= 1 {
        return Some("one character apart".into());
    }
    None
}

/// `b` is `a` with an English plural ending. Deliberately shallow — it catches
/// the cases that actually accumulate in a tag list and does not pretend to
/// know about "geese".
fn plural_of(a: &str, b: &str) -> bool {
    b == format!("{a}s")
        || b == format!("{a}es")
        || (a.ends_with('y') && b == format!("{}ies", &a[..a.len() - 1]))
}

/// Levenshtein, two rows rather than a full matrix.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Order-independent, so dismissing (a, b) also dismisses (b, a).
pub fn duplicate_key(a: &str, b: &str) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    format!("tagdup:{lo}:{hi}")
}

/* --------------------------------------------------- metadata proposals */

/// Format and Era for one item, from attributes the indexer already wrote.
///
/// Returns nothing for an item that already carries a tag in that facet: a
/// suggestion is for a gap, and re-proposing a facet you have filled is the
/// software second-guessing a decision you made.
pub fn for_node(conn: &Connection, node_id: &str) -> Result<Vec<MetadataSuggestion>> {
    let (node_type, content_type): (String, String) = conn.query_row(
        "SELECT node_type, content_type FROM node WHERE id = ?1",
        params![node_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if node_type == "tag" || node_type == "collector" {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let filled = filled_facets(conn, node_id)?;

    if !filled.iter().any(|f| f == "format") {
        if let Some(name) = format_name(&content_type) {
            out.push(MetadataSuggestion {
                key: format!("metatag:{node_id}:format:{name}"),
                facet: "format".into(),
                name,
                evidence: format!("content type {content_type}"),
            });
        }
    }

    if !filled.iter().any(|f| f == "era") {
        if let Some((decade, from)) = decade_of(conn, node_id)? {
            out.push(MetadataSuggestion {
                key: format!("metatag:{node_id}:era:{decade}"),
                facet: "era".into(),
                name: decade,
                evidence: from,
            });
        }
    }

    let mut kept = Vec::new();
    for s in out {
        if !is_dismissed(conn, &s.key)? {
            kept.push(s);
        }
    }
    debug_assert!(
        kept.iter().all(|s| facets::get(&s.facet).is_some_and(|f| f.machine_fillable)),
        "only tier 1 may be proposed from metadata"
    );
    Ok(kept)
}

fn filled_facets(conn: &Connection, node_id: &str) -> Result<Vec<String>> {
    let mut q = conn.prepare(
        "SELECT DISTINCT t.facet FROM edge e JOIN tag t ON t.node_id = e.target_id
          WHERE e.source_id = ?1 AND e.kind = 'tag_of'",
    )?;
    let out = q
        .query_map(params![node_id], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

/// A readable name for the leaf content type: `public.jpeg` becomes `JPEG`.
/// The tree is not consulted — "Image" would be a Format tag that says nothing
/// the icon does not already say.
fn format_name(content_type: &str) -> Option<String> {
    let leaf = content_type.rsplit('.').next()?;
    if leaf.is_empty() || content_type == "app.archiva.virtual" {
        return None;
    }
    Some(match leaf {
        "jpeg" => "JPEG".into(),
        "png" => "PNG".into(),
        "tiff" => "TIFF".into(),
        "heic" | "heif" => "HEIC".into(),
        "pdf" => "PDF".into(),
        "mpeg-4" | "mpeg4" => "MP4".into(),
        "quicktime-movie" => "QuickTime".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => return None,
            }
        }
    })
}

/// The decade a file is from, preferring when the photograph was taken over
/// when the file was written. A copied file has a new mtime and the same
/// capture date, so the capture date is the one that survives a backup.
fn decade_of(conn: &Connection, node_id: &str) -> Result<Option<(String, String)>> {
    let captured: Option<String> = conn
        .query_row(
            "SELECT value FROM attribute WHERE node_id = ?1 AND key = 'captured_at'",
            params![node_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(year) = captured.as_deref().and_then(year_in) {
        return Ok(Some((decade_label(year), "the date it was taken".into())));
    }
    let mtime: Option<String> = conn
        .query_row(
            "SELECT mtime FROM node WHERE id = ?1",
            params![node_id],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    Ok(mtime
        .as_deref()
        .and_then(year_in)
        .map(|y| (decade_label(y), "the date the file was written".into())))
}

/// The first four-digit run that could be a year. Works for `2024-06-11…`
/// and for the `2024:06:11 14:22:03` EXIF writes.
fn year_in(text: &str) -> Option<i32> {
    let digits: Vec<char> = text.chars().collect();
    for w in digits.windows(4) {
        if w.iter().all(char::is_ascii_digit) {
            let y: i32 = w.iter().collect::<String>().parse().ok()?;
            if (1826..=2999).contains(&y) {
                return Some(y);
            }
        }
    }
    None
}

fn decade_label(year: i32) -> String {
    format!("{}s", year - year.rem_euclid(10))
}

/* ------------------------------------------------------------ dismissal */

pub fn dismiss(conn: &Connection, key: &str, kind: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO dismissed(dismiss_key, kind) VALUES (?1, ?2)",
        params![key, kind],
    )?;
    Ok(())
}

pub fn is_dismissed(conn: &Connection, key: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dismissed WHERE dismiss_key = ?1",
        params![key],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    fn item(c: &Connection, id: &str, ct: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,locator,mtime)
             VALUES (?1,'media',?2,?1,?1,'2024-06-11T09:00:00Z')",
            params![id, ct],
        )
        .unwrap();
    }

    #[test]
    fn a_plural_and_a_typo_are_both_caught() {
        let c = db();
        tags::ensure(&c, "singer", "subject").unwrap();
        tags::ensure(&c, "singers", "subject").unwrap();
        tags::ensure(&c, "harbour", "environment").unwrap();
        tags::ensure(&c, "harbor", "environment").unwrap();
        let pairs = near_duplicates(&c).unwrap();
        assert_eq!(pairs.len(), 2);
        assert!(pairs.iter().any(|p| p.reason.contains("plural")));
        assert!(pairs.iter().any(|p| p.reason.contains("character")));
    }

    #[test]
    fn short_words_are_not_paired_on_one_character() {
        // "cat" and "car" are one edit apart and nothing to do with each other.
        let c = db();
        tags::ensure(&c, "cat", "subject").unwrap();
        tags::ensure(&c, "car", "subject").unwrap();
        assert!(near_duplicates(&c).unwrap().is_empty());
    }

    #[test]
    fn the_same_word_in_two_facets_is_never_offered_as_a_duplicate() {
        let c = db();
        tags::ensure(&c, "coastline", "environment").unwrap();
        tags::ensure(&c, "coastline", "subject").unwrap();
        assert!(
            near_duplicates(&c).unwrap().is_empty(),
            "two facets is a deliberate distinction, not a duplicate"
        );
    }

    #[test]
    fn dismissing_a_pair_keeps_both_permanently() {
        let c = db();
        tags::ensure(&c, "singer", "subject").unwrap();
        tags::ensure(&c, "singers", "subject").unwrap();
        let key = near_duplicates(&c).unwrap()[0].key.clone();
        dismiss(&c, &key, "tag_duplicate").unwrap();
        assert!(near_duplicates(&c).unwrap().is_empty());
        assert_eq!(tags::list(&c).unwrap().len(), 2);
    }

    #[test]
    fn a_dismissal_does_not_depend_on_which_way_round_the_pair_came() {
        assert_eq!(duplicate_key("b", "a"), duplicate_key("a", "b"));
    }

    #[test]
    fn the_busier_tag_is_offered_as_the_one_to_keep() {
        let c = db();
        item(&c, "i1", "public.jpeg");
        item(&c, "i2", "public.jpeg");
        let few = tags::ensure(&c, "singers", "subject").unwrap();
        let many = tags::ensure(&c, "singer", "subject").unwrap();
        tags::apply(&c, &["i1".into(), "i2".into()], &many).unwrap();
        tags::apply(&c, &["i1".into()], &few).unwrap();
        let p = &near_duplicates(&c).unwrap()[0];
        assert_eq!(p.a.name, "singer");
        assert_eq!(p.b.name, "singers");
    }

    #[test]
    fn format_and_era_are_proposed_and_nothing_below_tier_one_is() {
        let c = db();
        item(&c, "i1", "public.jpeg");
        let s = for_node(&c, "i1").unwrap();
        let facets: Vec<&str> = s.iter().map(|x| x.facet.as_str()).collect();
        assert_eq!(facets, vec!["format", "era"]);
        assert_eq!(s[0].name, "JPEG");
        assert_eq!(s[1].name, "2020s");
        assert!(s[1].evidence.contains("file was written"));
    }

    #[test]
    fn the_capture_date_beats_the_file_date() {
        let c = db();
        item(&c, "i1", "public.jpeg");
        c.execute(
            "INSERT INTO attribute(node_id,key,value) VALUES ('i1','captured_at','1978:04:02 11:00:00')",
            [],
        )
        .unwrap();
        let s = for_node(&c, "i1").unwrap();
        let era = s.iter().find(|x| x.facet == "era").unwrap();
        assert_eq!(era.name, "1970s");
        assert!(era.evidence.contains("taken"));
    }

    #[test]
    fn a_facet_already_filled_is_not_proposed_again() {
        let c = db();
        item(&c, "i1", "public.jpeg");
        let t = tags::ensure(&c, "Polaroid", "format").unwrap();
        tags::apply(&c, &["i1".into()], &t).unwrap();
        let s = for_node(&c, "i1").unwrap();
        assert!(s.iter().all(|x| x.facet != "format"));
    }

    #[test]
    fn a_dismissed_proposal_does_not_come_back() {
        let c = db();
        item(&c, "i1", "public.jpeg");
        let key = for_node(&c, "i1").unwrap()[0].key.clone();
        dismiss(&c, &key, "metadata_tag").unwrap();
        let s = for_node(&c, "i1").unwrap();
        assert!(s.iter().all(|x| x.facet != "format"));
    }

    #[test]
    fn tags_and_collectors_are_never_offered_suggestions() {
        let c = db();
        let t = tags::ensure(&c, "coast", "environment").unwrap();
        assert!(for_node(&c, &t).unwrap().is_empty());
    }

    #[test]
    fn a_year_is_found_wherever_the_date_format_puts_it() {
        assert_eq!(year_in("2024-06-11T09:00:00Z"), Some(2024));
        assert_eq!(year_in("1978:04:02 11:00:00"), Some(1978));
        assert_eq!(year_in("no date here"), None);
        assert_eq!(decade_label(1978), "1970s");
        assert_eq!(decade_label(2020), "2020s");
    }

    #[test]
    fn edit_distance_is_the_usual_one() {
        assert_eq!(edit_distance("harbour", "harbor"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }
}
