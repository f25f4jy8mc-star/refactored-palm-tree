//! Proposals. Never applications.
//!
//! Checklist C4 and C5, and principle 3: the machine suggests, the user
//! classifies. Nothing in this file writes a tag or an edge. It returns things
//! a person can accept, and remembers what they waved away.
//!
//! Three kinds, all tier 1 of the suggestion ladder — arithmetic on what is
//! already stored, with no machine learning anywhere:
//!
//!   * **near-duplicate tags** (C4): two tags in one facet that differ by a
//!     character or a plural. Offered as a merge.
//!   * **metadata suggestions** (C5): Format and Era read off what the file
//!     already says about itself. Tier 1 is the only tier that can be
//!     proposed this way — Environment, Action, Attribute and Subject are
//!     judgements, and a machine reading them off a file is a machine
//!     classifying.
//!   * **vocabulary suggestions**: a tag you already coined, offered for an
//!     item that does not carry it, because items like it do or because the
//!     word is in its name. Any facet, and the paragraph on
//!     `from_vocabulary` says why that is not the same licence as above.
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

use std::collections::HashMap;

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
pub struct TagSuggestion {
    pub key: String,
    /// Which ladder rung produced this — `metadata_tag` or `vocabulary_tag`.
    /// Carried so dismissing one does not need the view to know where it came
    /// from; the `dismissed` table is keyed by both (G19).
    pub kind: String,
    pub facet: String,
    pub name: String,
    /// What produced this, in the words a person would use. Shown, because an
    /// unexplained suggestion is indistinguishable from a guess.
    pub evidence: String,
}

pub const FROM_METADATA: &str = "metadata_tag";
pub const FROM_VOCABULARY: &str = "vocabulary_tag";

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
pub fn for_node(conn: &Connection, node_id: &str) -> Result<Vec<TagSuggestion>> {
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
            out.push(TagSuggestion {
                key: format!("metatag:{node_id}:format:{name}"),
                kind: FROM_METADATA.into(),
                facet: "format".into(),
                name,
                evidence: format!("content type {content_type}"),
            });
        }
    }

    if !filled.iter().any(|f| f == "era") {
        if let Some((decade, from)) = decade_of(conn, node_id)? {
            out.push(TagSuggestion {
                key: format!("metatag:{node_id}:era:{decade}"),
                kind: FROM_METADATA.into(),
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
    // Archiva's own types are not formats. The leaf of
    // `app.archiva.note.file` is "file", and "File as format" is a
    // suggestion that says nothing — except for a note on disk, which really
    // is markdown and is worth offering as such.
    if content_type == "app.archiva.note.file" {
        return Some("Markdown".into());
    }
    if content_type.starts_with("app.archiva.") {
        return None;
    }
    let leaf = content_type.rsplit('.').next()?;
    if leaf.is_empty() {
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
        // The leaf of an audio type is a mouthful nobody writes on a tag:
        // "com.microsoft.waveform-audio" is a WAV.
        "waveform-audio" => "WAV".into(),
        "aiff-audio" => "AIFF".into(),
        "mp3" | "mpeg-3-audio" => "MP3".into(),
        "aac-audio" => "AAC".into(),
        "flac" => "FLAC".into(),
        "ogg" => "OGG".into(),
        "wavefront-obj" => "OBJ".into(),
        "gltf" => "glTF".into(),
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
        .and_then(year_of_mtime)
        .map(|y| (decade_label(y), "the date the file was written".into())))
}

/// The year in a `node.mtime`, which is **not** a date string: `signals`
/// stores microseconds since the epoch, zero-padded to sixteen digits, so
/// that string and numeric comparison agree.
///
/// Mining four-digit runs out of that (which is what `year_in` does, rightly,
/// for the EXIF text in `captured_at`) finds whatever the clock happens to
/// spell. A file written last week offered "2650s" as its era — a suggestion
/// with a plausible shape and no meaning, which is worse than none.
///
/// Anything that is not a bare integer is read as text, so a row written by
/// an older build or by the backfill is still understood rather than silently
/// giving up its era.
fn year_of_mtime(mtime: &str) -> Option<i32> {
    match mtime.trim().parse::<i64>() {
        Ok(micros) => Some(civil_year(micros.div_euclid(86_400_000_000))),
        Err(_) => year_in(mtime),
    }
}

/// The year containing the given count of days from 1970-01-01. Hinnant's
/// civil-from-days, shortened to the part that answers a year — exact, and
/// cheaper than taking on a date crate for one question.
fn civil_year(days_from_epoch: i64) -> i32 {
    // Shift the epoch to 0000-03-01 so leap days land at the end of a cycle.
    let z = days_from_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // day of era, 0..=146096
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    // March-based year: January and February belong to the next civil year.
    let mp = (5 * doy + 2) / 153;
    (y + i64::from(mp >= 10)) as i32
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

/* ------------------------------------------------- vocabulary proposals */

/// How many other items must agree before a co-occurrence is worth offering.
/// One is a coincidence; the point of counting is to find a habit.
const CO_OCCURRENCE_FLOOR: i64 = 2;

/// Short words pair by accident. A three-letter tag matching a filename is
/// noise more often than not.
const MIN_NAME_MATCH: usize = 4;

/// At most this many, so the panel stays a prompt rather than a queue.
const MAX_VOCABULARY: usize = 6;

/// Tags you already use that could apply to this item too.
///
/// Tier 1 of the suggestion ladder and arithmetic only, on two signals the
/// checklist names by hand (L2 — "edit distance and tag co-occurrence"):
///
///   * **co-occurrence** — items carrying a tag this one has usually carry
///     another one as well, and this item does not have it;
///   * **the item's own name** — a word in it is already a tag.
///
/// Nothing here is invented. Every proposal is a tag *you* coined, offered
/// for an item that does not carry it, with the count or the word that
/// produced it shown beside it.
///
/// **Why this may touch judgement facets where `for_node` may not.** Reading
/// a Subject off a photograph's pixels would be the machine deciding what the
/// photograph is about — that is the line `for_node` holds, and it still
/// holds it. Noticing that forty items tagged "harbour" are also tagged
/// "boats", and asking whether this one is too, is the machine reading your
/// filing rather than the file. The accept-only rule is unchanged: nothing
/// below writes anything.
pub fn from_vocabulary(conn: &Connection, node_id: &str) -> Result<Vec<TagSuggestion>> {
    let node_type: String = conn.query_row(
        "SELECT node_type FROM node WHERE id = ?1",
        params![node_id],
        |r| r.get(0),
    )?;
    // A tag is not a thing you tag.
    if node_type == "tag" {
        return Ok(Vec::new());
    }

    let held = tags::of_node(conn, node_id)?;
    let held_ids: Vec<&str> = held.iter().map(|t| t.id.as_str()).collect();

    // tag id -> (strength, evidence). A tag reached both ways keeps the
    // stronger reason, which is always the counted one.
    let mut found: HashMap<String, (i64, String)> = HashMap::new();

    for mine in &held {
        for (tag_id, n) in alongside(conn, node_id, &mine.id)? {
            if held_ids.contains(&tag_id.as_str()) {
                continue;
            }
            let evidence = format!("on {n} other items tagged “{}”", mine.name);
            found
                .entry(tag_id)
                .and_modify(|e| {
                    if n > e.0 {
                        *e = (n, evidence.clone());
                    }
                })
                .or_insert((n, evidence));
        }
    }

    let name: String = conn.query_row(
        "SELECT display_name FROM node WHERE id = ?1",
        params![node_id],
        |r| r.get(0),
    )?;
    let words = words_of(&name);
    for tag in tags::list(conn)? {
        if held_ids.contains(&tag.id.as_str()) || found.contains_key(&tag.id) {
            continue;
        }
        let key = tags::normalise(&tag.name);
        if key.chars().count() < MIN_NAME_MATCH {
            continue;
        }
        if words.iter().any(|w| w == &key) {
            // Strength 0, so every counted suggestion outranks a name match:
            // a habit across the library is better evidence than a filename.
            found.insert(tag.id.clone(), (0, format!("“{}” is in its name", tag.name)));
        }
    }

    let by_id: HashMap<String, Tag> = tags::list(conn)?
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();

    let mut out: Vec<(i64, TagSuggestion)> = Vec::new();
    for (tag_id, (strength, evidence)) in found {
        let Some(tag) = by_id.get(&tag_id) else { continue };
        let key = format!("vocabtag:{node_id}:{tag_id}");
        if is_dismissed(conn, &key)? {
            continue;
        }
        out.push((
            strength,
            TagSuggestion {
                key,
                kind: FROM_VOCABULARY.into(),
                facet: tag.facet.clone(),
                name: tag.name.clone(),
                evidence,
            },
        ));
    }
    // Strongest first, then by name so the list is stable between reads —
    // a panel that reshuffles on every refresh is unusable.
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    Ok(out.into_iter().take(MAX_VOCABULARY).map(|(_, s)| s).collect())
}

/// Tags carried by other items that also carry `tag_id`, with how many other
/// items that is. Only counts that clear the floor come back.
fn alongside(conn: &Connection, node_id: &str, tag_id: &str) -> Result<Vec<(String, i64)>> {
    let mut q = conn.prepare(
        "SELECT other.target_id, COUNT(DISTINCT other.source_id)
           FROM edge mine
           JOIN edge other
             ON other.source_id = mine.source_id AND other.kind = 'tag_of'
          WHERE mine.kind = 'tag_of'
            AND mine.target_id = ?1
            AND mine.source_id <> ?2
            AND other.target_id <> ?1
          GROUP BY other.target_id
         HAVING COUNT(DISTINCT other.source_id) >= ?3",
    )?;
    let out = q
        .query_map(params![tag_id, node_id, CO_OCCURRENCE_FLOOR], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

/// The words in a name, lowercased. Splits on anything that is not a letter
/// or a digit, so `harbour-wall_02.jpg` offers `harbour`, `wall`, `02`, `jpg`.
fn words_of(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
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

    /// `mtime` is what `signals::fmt_time` writes — microseconds since the
    /// epoch, padded to sixteen digits. It used to be an ISO string here,
    /// which is a shape the scanner never produces, and the era test passed
    /// against it while the real thing offered "2650s".
    fn item(c: &Connection, id: &str, ct: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,locator,mtime)
             VALUES (?1,'media',?2,?1,?1,'1718100000000000')",
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
    fn an_mtime_is_read_as_the_stamp_it_is_rather_than_mined_for_digits() {
        // `signals::fmt_time` writes microseconds since the epoch, padded to
        // sixteen digits. Reading four-digit runs out of that is how a file
        // written last week came to suggest "2650s".
        assert_eq!(year_of_mtime("0000000000000000"), Some(1970));
        assert_eq!(year_of_mtime("1718100000000000"), Some(2024));
        assert_eq!(year_of_mtime("0252460800000000"), Some(1978));
        assert_eq!(year_of_mtime("not a stamp"), None);
        // A row from an older build, or from the backfill, is still read.
        assert_eq!(year_of_mtime("2024-06-11T09:00:00Z"), Some(2024));

        // The boundary the cheap "days / 365.25" version gets wrong.
        assert_eq!(civil_year(0), 1970);
        assert_eq!(civil_year(-1), 1969, "31 December 1969");
        assert_eq!(civil_year(365), 1971, "1 January 1971");
        assert_eq!(civil_year(18_993), 2022, "1 January 2022");
        assert_eq!(civil_year(18_992), 2021, "31 December 2021");
    }

    #[test]
    fn the_era_proposed_from_a_file_date_is_the_files_real_decade() {
        let c = db();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,locator,mtime)
             VALUES ('n','media','public.jpeg','n','n','1718100000000000')",
            [],
        )
        .unwrap();
        let out = for_node(&c, "n").unwrap();
        let era = out.iter().find(|s| s.facet == "era").expect("an era");
        assert_eq!(era.name, "2020s");
    }

    #[test]
    fn archivas_own_types_are_not_offered_as_formats() {
        // The leaf of `app.archiva.note.file` is "file", and "File as format"
        // is a suggestion that says nothing.
        assert_eq!(format_name("app.archiva.note.file").as_deref(), Some("Markdown"));
        assert_eq!(format_name("app.archiva.virtual"), None);
        assert_eq!(format_name("app.archiva.collector.folder"), None);
        assert_eq!(format_name("public.jpeg").as_deref(), Some("JPEG"));
        // And a leaf nobody would write on a tag gets the name people use.
        assert_eq!(format_name("com.microsoft.waveform-audio").as_deref(), Some("WAV"));
        assert_eq!(format_name("public.wavefront-obj").as_deref(), Some("OBJ"));
    }

    #[test]
    fn edit_distance_is_the_usual_one() {
        assert_eq!(edit_distance("harbour", "harbor"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    /* ------------------------------------------------ vocabulary */

    fn named(c: &Connection, id: &str, name: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,locator)
             VALUES (?1,'media','public.jpeg',?2,?1)",
            params![id, name],
        )
        .unwrap();
    }

    fn tagged(c: &Connection, node: &str, name: &str, facet: &str) -> String {
        let tag = tags::ensure(c, name, facet).unwrap();
        tags::apply(c, &[node.to_string()], &tag).unwrap();
        tag
    }

    fn names(out: &[TagSuggestion]) -> Vec<&str> {
        out.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn a_tag_that_keeps_company_with_one_you_have_is_offered() {
        // Three other items say harbour goes with boats. This one has
        // harbour and not boats, so boats is worth asking about.
        let c = db();
        for i in 1..=3 {
            let id = format!("other{i}");
            named(&c, &id, &id);
            tagged(&c, &id, "harbour", "environment");
            tagged(&c, &id, "boats", "subject");
        }
        named(&c, "mine", "DSC_0041");
        tagged(&c, "mine", "harbour", "environment");

        let out = from_vocabulary(&c, "mine").unwrap();
        assert_eq!(names(&out), vec!["boats"]);
        assert_eq!(out[0].facet, "subject");
        assert_eq!(out[0].kind, FROM_VOCABULARY);
        assert_eq!(
            out[0].evidence, "on 3 other items tagged “harbour”",
            "the count and the tag that produced it, both shown"
        );
    }

    #[test]
    fn one_other_item_is_a_coincidence_rather_than_a_habit() {
        let c = db();
        named(&c, "other", "other");
        tagged(&c, "other", "harbour", "environment");
        tagged(&c, "other", "boats", "subject");
        named(&c, "mine", "mine");
        tagged(&c, "mine", "harbour", "environment");

        assert!(from_vocabulary(&c, "mine").unwrap().is_empty());
    }

    #[test]
    fn a_tag_already_on_the_item_is_never_offered_back() {
        let c = db();
        for i in 1..=3 {
            let id = format!("other{i}");
            named(&c, &id, &id);
            tagged(&c, &id, "harbour", "environment");
            tagged(&c, &id, "boats", "subject");
        }
        named(&c, "mine", "mine");
        tagged(&c, "mine", "harbour", "environment");
        tagged(&c, "mine", "boats", "subject");

        assert!(from_vocabulary(&c, "mine").unwrap().is_empty());
    }

    #[test]
    fn a_word_in_the_name_that_is_already_a_tag_is_offered() {
        let c = db();
        tags::ensure(&c, "Bergamo", "subject").unwrap();
        named(&c, "mine", "bergamo-arcade_02");

        let out = from_vocabulary(&c, "mine").unwrap();
        assert_eq!(names(&out), vec!["Bergamo"]);
        assert_eq!(out[0].evidence, "“Bergamo” is in its name");
    }

    #[test]
    fn a_short_tag_does_not_match_a_filename_by_accident() {
        // "raw" would otherwise fire on every file whose name happens to
        // contain it, which is how a suggestion becomes noise.
        let c = db();
        tags::ensure(&c, "raw", "format").unwrap();
        named(&c, "mine", "raw-harbour");
        assert!(from_vocabulary(&c, "mine").unwrap().is_empty());
    }

    #[test]
    fn a_counted_suggestion_outranks_a_name_match() {
        let c = db();
        for i in 1..=3 {
            let id = format!("other{i}");
            named(&c, &id, &id);
            tagged(&c, &id, "harbour", "environment");
            tagged(&c, &id, "boats", "subject");
        }
        tags::ensure(&c, "Bergamo", "subject").unwrap();
        named(&c, "mine", "bergamo-harbour");
        tagged(&c, "mine", "harbour", "environment");

        let out = from_vocabulary(&c, "mine").unwrap();
        assert_eq!(
            names(&out),
            vec!["boats", "Bergamo"],
            "a habit across the library beats a word in one filename"
        );
    }

    #[test]
    fn nothing_is_offered_from_an_empty_vocabulary() {
        let c = db();
        named(&c, "mine", "harbour-wall");
        assert!(from_vocabulary(&c, "mine").unwrap().is_empty());
    }

    #[test]
    fn dismissing_one_keeps_it_away() {
        let c = db();
        tags::ensure(&c, "Bergamo", "subject").unwrap();
        named(&c, "mine", "bergamo-arcade");

        let out = from_vocabulary(&c, "mine").unwrap();
        assert_eq!(out.len(), 1);
        dismiss(&c, &out[0].key, &out[0].kind).unwrap();
        assert!(from_vocabulary(&c, "mine").unwrap().is_empty());
    }

    #[test]
    fn a_judgement_facet_is_reachable_here_though_not_from_metadata() {
        // The line `for_node` holds is about reading a file: a Subject taken
        // off a photograph's pixels is the machine classifying. A Subject you
        // coined, counted across your own filing, is not the same claim.
        let c = db();
        for i in 1..=2 {
            let id = format!("other{i}");
            named(&c, &id, &id);
            tagged(&c, &id, "harbour", "environment");
            tagged(&c, &id, "fishing", "action");
        }
        named(&c, "mine", "mine");
        tagged(&c, "mine", "harbour", "environment");

        let out = from_vocabulary(&c, "mine").unwrap();
        assert_eq!(out[0].facet, "action");
        assert!(
            !facets::get("action").unwrap().machine_fillable,
            "the facet metadata may not fill"
        );
    }

    #[test]
    fn a_tag_is_not_something_to_tag() {
        let c = db();
        let t = tags::ensure(&c, "coast", "environment").unwrap();
        assert!(from_vocabulary(&c, &t).unwrap().is_empty());
    }

    #[test]
    fn the_list_is_capped_so_the_panel_stays_a_prompt() {
        let c = db();
        for i in 1..=3 {
            let id = format!("other{i}");
            named(&c, &id, &id);
            tagged(&c, &id, "harbour", "environment");
            for w in ["boats", "nets", "rope", "salt", "gulls", "piers", "tide"] {
                tagged(&c, &id, w, "subject");
            }
        }
        named(&c, "mine", "mine");
        tagged(&c, "mine", "harbour", "environment");

        assert_eq!(from_vocabulary(&c, "mine").unwrap().len(), MAX_VOCABULARY);
    }
}
