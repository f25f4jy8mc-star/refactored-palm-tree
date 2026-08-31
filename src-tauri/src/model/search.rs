//! `p_search` — name, body and tag-association hits in one call (G18).
//!
//! The palette's Build 17 predecessor made two calls (`searchNodes` +
//! `relatedNodes`) and merged them itself; the FTS5 index already carries all
//! three columns (`title`, `body`, `tags`), so one query per column here does
//! the merging once, in the one place every future caller reads from.
//!
//! Results are **sectioned by `match_kind`**, not globally re-ranked across
//! kinds — a title hit always orients the user faster than a body hit, so
//! every title hit precedes every body hit, and each section is internally
//! ordered by FTS5's own relevance score. This mirrors `p_rows`' own rule
//! (group, then sort within the group): the caller should never have to
//! re-sort what it's handed.
//!
//! `type_filter` is a conformance check against `content_type_tree` (G17),
//! never a string comparison against a subtype — the same rule `p_rows`'
//! grouping and the palette's `⌥i`/`⌥v`/`⌥3` filters already follow.

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::content_type;
use super::projections::{row, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    Name,
    Body,
    ViaTag,
}

#[derive(Debug, Serialize)]
pub struct Hit {
    pub node: Row,
    pub match_kind: MatchKind,
    /// Plain text with `‹…›` around the matched term, since a view has no
    /// other way to say *why* a result matched.
    pub snippet: String,
}

pub struct Options {
    /// A conformance ancestor, e.g. `public.image` — never a subtype string.
    pub type_filter: Option<String>,
    pub limit: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            type_filter: None,
            limit: 50,
        }
    }
}

/// Every token becomes a quoted prefix match, ANDed together, so a search
/// term containing FTS5 syntax characters (`-`, `:`, `"`) is treated as text
/// rather than a broken query. `None` for an empty/whitespace-only query —
/// there is nothing to search for and no result should be manufactured.
fn fts_terms(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

fn column_index(column: &str) -> i64 {
    // Position within `search(node_id, title, body, tags)` — node_id is
    // UNINDEXED but still occupies column 0.
    match column {
        "title" => 1,
        "body" => 2,
        _ => 3,
    }
}

fn column_hits(conn: &Connection, column: &str, terms: &str, limit: i64) -> Result<Vec<(String, String)>> {
    let sql = format!(
        "SELECT node_id, snippet(search, {idx}, '\u{2039}', '\u{203a}', '\u{2026}', 10)
           FROM search WHERE search MATCH ?1
          ORDER BY bm25(search) LIMIT ?2",
        idx = column_index(column),
    );
    let match_expr = format!("{column}:({terms})");
    let mut q = conn.prepare(&sql)?;
    let out = q
        .query_map(params![match_expr, limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(out)
}

pub fn search(conn: &Connection, query: &str, opts: &Options) -> Result<Vec<Hit>> {
    let Some(terms) = fts_terms(query) else {
        return Ok(Vec::new());
    };
    let limit = opts.limit.max(1) as i64;
    let mut seen: HashSet<String> = HashSet::new();
    let mut hits = Vec::new();

    // Name beats body beats tag-association (see module docs), so the
    // sections are built and appended in that fixed order rather than
    // re-ranked afterwards.
    //
    // `tags` is reserved in the schema for the via-tag section (G18), but
    // nothing writes to it yet: applying a tag_of edge doesn't sync
    // `search.tags` today, so a via-tag match can never fire. That's a
    // mutation-side gap (`mutations::link`), not this projection's — when it
    // lands, this loop only needs `("tags", MatchKind::ViaTag)` added.
    for (column, kind) in [("title", MatchKind::Name), ("body", MatchKind::Body)] {
        for (node_id, snippet) in column_hits(conn, column, &terms, limit)? {
            if seen.contains(&node_id) {
                continue;
            }
            if let Some(filter) = &opts.type_filter {
                let ct: String = conn.query_row(
                    "SELECT content_type FROM node WHERE id = ?1",
                    params![node_id],
                    |r| r.get(0),
                )?;
                if !content_type::conforms_to(&ct, filter) {
                    continue;
                }
            }
            seen.insert(node_id.clone());
            hits.push(Hit {
                node: row(conn, &node_id)?,
                match_kind: kind,
                snippet,
            });
        }
    }

    hits.truncate(opts.limit);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        let mk = |id: &str, ct: &str, name: &str| {
            c.execute(
                "INSERT INTO node(id, node_type, content_type, content_type_tree, display_name, icon_kind)
                 VALUES (?1, 'media', ?2, '[]', ?3, 'x')",
                params![id, ct, name],
            )
            .unwrap();
        };
        mk("n1", "public.jpeg", "Bergamo arcade");
        mk("n2", "com.adobe.pdf", "Receipts");
        c.execute(
            "UPDATE search SET body = 'the arcade in Bergamo was closed' WHERE node_id = 'n2'",
            [],
        )
        .unwrap();
        c
    }

    #[test]
    fn a_title_match_and_a_body_match_both_surface_sectioned_by_kind() {
        let c = seed();
        let hits = search(&c, "arcade", &Options::default()).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].node.id, "n1");
        assert_eq!(hits[0].match_kind, MatchKind::Name);
        assert_eq!(hits[1].node.id, "n2");
        assert_eq!(hits[1].match_kind, MatchKind::Body);
    }

    #[test]
    fn a_node_matching_both_title_and_body_is_reported_once_as_a_name_hit() {
        let c = seed();
        c.execute(
            "UPDATE search SET body = 'bergamo again' WHERE node_id = 'n1'",
            [],
        )
        .unwrap();
        let hits = search(&c, "bergamo", &Options::default()).unwrap();
        let n1_hits: Vec<_> = hits.iter().filter(|h| h.node.id == "n1").collect();
        assert_eq!(n1_hits.len(), 1, "n1 must not appear twice");
        assert_eq!(n1_hits[0].match_kind, MatchKind::Name);
    }

    #[test]
    fn the_snippet_marks_the_matched_term() {
        let c = seed();
        let hits = search(&c, "receipts", &Options::default()).unwrap();
        assert!(hits[0].snippet.contains('\u{2039}'));
    }

    #[test]
    fn type_filter_is_a_conformance_check_not_a_string_match() {
        let c = seed();
        let opts = Options {
            type_filter: Some("public.image".into()),
            ..Options::default()
        };
        let hits = search(&c, "arcade", &opts).unwrap();
        assert_eq!(hits.len(), 1, "the PDF hit on 'arcade' in its body must be excluded");
        assert_eq!(hits[0].node.id, "n1");
    }

    #[test]
    fn limit_truncates_the_combined_result() {
        let c = seed();
        let opts = Options {
            limit: 1,
            ..Options::default()
        };
        let hits = search(&c, "arcade", &opts).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn fts5_syntax_characters_in_the_query_do_not_error() {
        let c = seed();
        for q in ["c++", "\"quoted\"", "-dash", "colon:here", ""] {
            assert!(search(&c, q, &Options::default()).is_ok(), "query {q:?} failed");
        }
    }

    #[test]
    fn a_blank_query_returns_nothing_rather_than_everything() {
        let c = seed();
        assert_eq!(search(&c, "   ", &Options::default()).unwrap().len(), 0);
    }
}
