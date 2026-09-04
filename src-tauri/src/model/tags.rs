//! Tags: making them, applying them, and managing them.
//!
//! Checklist C2, C3 and C6. Three separate jobs that Build 17 ran together and
//! this file keeps apart:
//!
//!   * **applying** a tag to items — an edge, so it goes through the named
//!     write path in `mutations`, never a second INSERT written here;
//!   * **managing** the tags themselves — rename, delete, merge, reorder,
//!     which touch the tag node and never an item;
//!   * **promoting** a tag to a Collector, which is deliberately one-way.
//!
//! Everything that applies or removes a tag takes a **slice of node ids**
//! rather than one. Batch is not an optimisation here: classification fatigue
//! is the named risk that kills the library at item forty, and tagging forty
//! things one at a time is how you get there. A single-item call is a slice of
//! length one, so there is no second code path to keep in step.
//!
//! A tag is a node like everything else — `node_type = 'tag'`, with the `tag`
//! table carrying its facet and tier. It has no locator, which is why the
//! unique index on locator is partial.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use super::content_type;
use super::facets;
use super::mutations;
use super::scan::uuid_v7;

#[derive(Debug, Clone, Serialize)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub facet: String,
    pub tier: i64,
    pub sort_order: i64,
    /// How many items carry this tag. Derived, never stored — a stored count
    /// is a second copy of what the edges already know.
    pub usage: i64,
}

/// Casefolded and whitespace-collapsed, for comparing two names without
/// deciding they are the same thing. Used to keep `ensure` idempotent and by
/// `suggest::near_duplicates`; it is deliberately *not* stored, because a
/// stored normalisation goes stale the moment the rule changes.
pub fn normalise(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/* ------------------------------------------------------------ reading */

pub fn list(conn: &Connection) -> Result<Vec<Tag>> {
    let mut q = conn.prepare(
        "SELECT n.id, n.display_name, t.facet, t.tier, t.sort_order,
                (SELECT COUNT(*) FROM edge e
                  WHERE e.target_id = n.id AND e.kind = 'tag_of')
           FROM node n JOIN tag t ON t.node_id = n.id
          ORDER BY t.tier, t.facet, t.sort_order, n.display_name COLLATE NOCASE",
    )?;
    let out = q
        .query_map([], |r| {
            Ok(Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                facet: r.get(2)?,
                tier: r.get(3)?,
                sort_order: r.get(4)?,
                usage: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

/// The tags carried by one item, in the order a reader should meet them:
/// tier 1 first, so the structural facts come before the interpretive ones.
pub fn of_node(conn: &Connection, node_id: &str) -> Result<Vec<Tag>> {
    let mut q = conn.prepare(
        "SELECT n.id, n.display_name, t.facet, t.tier, t.sort_order,
                (SELECT COUNT(*) FROM edge e2
                  WHERE e2.target_id = n.id AND e2.kind = 'tag_of')
           FROM edge e
           JOIN node n ON n.id = e.target_id
           JOIN tag  t ON t.node_id = n.id
          WHERE e.source_id = ?1 AND e.kind = 'tag_of'
          ORDER BY t.tier, t.facet, t.sort_order, n.display_name COLLATE NOCASE",
    )?;
    let out = q
        .query_map(params![node_id], |r| {
            Ok(Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                facet: r.get(2)?,
                tier: r.get(3)?,
                sort_order: r.get(4)?,
                usage: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(out)
}

/* ------------------------------------------------------------ creating */

/// Make a tag, or return the one already there.
///
/// Idempotent on (normalised name, facet). The same word may legitimately be
/// an Environment and a Subject — "coastline" the place and "coastline" the
/// thing pictured are different claims — so the facet is part of the identity.
/// Two tags with the same name in *different* facets is fine; two in the same
/// facet is the duplicate C4 exists to catch, and this stops it at the source.
pub fn ensure(conn: &Connection, name: &str, facet: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("a tag needs a name"));
    }
    let tier = facets::tier_of(facet).ok_or_else(|| anyhow!("not a facet: {facet}"))?;

    let key = normalise(name);
    let existing: Option<String> = conn
        .query_row(
            "SELECT n.id FROM node n JOIN tag t ON t.node_id = n.id
              WHERE t.facet = ?1 AND LOWER(TRIM(n.display_name)) = ?2",
            params![facet, key],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = existing {
        return Ok(id);
    }

    let id = uuid_v7();
    conn.execute(
        "INSERT INTO node(id, node_type, content_type, content_type_tree,
                          title, display_name, icon_kind, source_kind)
         VALUES (?1, 'tag', ?2, ?3, ?4, ?4, 'tag', 'app_generated')",
        params![
            id,
            content_type::VIRTUAL,
            serde_json::to_string(&content_type::closure(content_type::VIRTUAL))?,
            name,
        ],
    )?;
    // Last in its facet, so a new tag never displaces one you have already
    // dragged into place.
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM tag WHERE facet = ?1",
        params![facet],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO tag(node_id, facet, tier, sort_order) VALUES (?1, ?2, ?3, ?4)",
        params![id, facet, tier, next],
    )?;
    Ok(id)
}

/* ------------------------------------------------------------ applying */

/// Apply one tag to many items. Returns how many items actually changed —
/// items that already carried it are not counted, and are not an error.
pub fn apply(conn: &Connection, node_ids: &[String], tag_id: &str) -> Result<usize> {
    require_tag(conn, tag_id)?;
    let mut changed = 0;
    for node_id in node_ids {
        // "N" rather than "tag_of": the write path derives the kind from what
        // it is pointed at, so a tag dropped on an item can only ever become a
        // tag_of edge (G7). Passing the kind in here would be a second place
        // that decides, and the two would drift.
        let linked = mutations::link(conn, node_id, "N", tag_id, None, None)?;
        if !linked.existed {
            changed += 1;
        }
    }
    Ok(changed)
}

/// Take one tag off many items. Returns how many carried it.
pub fn unapply(conn: &Connection, node_ids: &[String], tag_id: &str) -> Result<usize> {
    let mut changed = 0;
    for node_id in node_ids {
        let edge: Option<String> = conn
            .query_row(
                "SELECT id FROM edge
                  WHERE source_id = ?1 AND target_id = ?2 AND kind = 'tag_of'",
                params![node_id, tag_id],
                |r| r.get(0),
            )
            .ok();
        if let Some(edge_id) = edge {
            mutations::unlink(conn, &edge_id)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/* ------------------------------------------------------------ managing */

pub fn rename(conn: &Connection, tag_id: &str, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("a tag needs a name"));
    }
    require_tag(conn, tag_id)?;
    let facet: String = conn.query_row(
        "SELECT facet FROM tag WHERE node_id = ?1",
        params![tag_id],
        |r| r.get(0),
    )?;
    let clash: Option<String> = conn
        .query_row(
            "SELECT n.id FROM node n JOIN tag t ON t.node_id = n.id
              WHERE t.facet = ?1 AND LOWER(TRIM(n.display_name)) = ?2 AND n.id <> ?3",
            params![facet, normalise(name), tag_id],
            |r| r.get(0),
        )
        .ok();
    if clash.is_some() {
        return Err(anyhow!(
            "there is already a {facet} tag called “{name}” — merge them instead"
        ));
    }
    conn.execute(
        "UPDATE node SET display_name = ?2, title = ?2, modified_at = datetime('now')
          WHERE id = ?1",
        params![tag_id, name],
    )?;
    Ok(())
}

/// Move a tag to a different facet. The tier follows the facet — it is never
/// passed in, because a caller that can choose the tier is a caller that can
/// put a Subject in tier 1.
pub fn set_facet(conn: &Connection, tag_id: &str, facet: &str) -> Result<()> {
    require_tag(conn, tag_id)?;
    let tier = facets::tier_of(facet).ok_or_else(|| anyhow!("not a facet: {facet}"))?;
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM tag WHERE facet = ?1",
        params![facet],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE tag SET facet = ?2, tier = ?3, sort_order = ?4 WHERE node_id = ?1",
        params![tag_id, facet, tier, next],
    )?;
    Ok(())
}

/// Delete a tag outright. Every `tag_of` edge into it goes with it, by
/// cascade — the items themselves are untouched.
pub fn delete(conn: &Connection, tag_id: &str) -> Result<usize> {
    require_tag(conn, tag_id)?;
    let carried: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edge WHERE target_id = ?1 AND kind = 'tag_of'",
        params![tag_id],
        |r| r.get(0),
    )?;
    conn.execute("DELETE FROM node WHERE id = ?1", params![tag_id])?;
    Ok(carried as usize)
}

/// Fold `from` into `into`. Every item carrying `from` ends up carrying
/// `into`, and `from` is deleted.
///
/// `UPDATE OR IGNORE` rather than a loop: an item carrying both tags would
/// otherwise collide with the unique index on (source, target, kind), and the
/// row that fails to move is exactly the row that no longer needs to.
pub fn merge(conn: &Connection, from: &str, into: &str) -> Result<usize> {
    if from == into {
        return Err(anyhow!("a tag cannot be merged into itself"));
    }
    require_tag(conn, from)?;
    require_tag(conn, into)?;
    let moved = conn.execute(
        "UPDATE OR IGNORE edge SET target_id = ?2
          WHERE target_id = ?1 AND kind = 'tag_of'",
        params![from, into],
    )?;
    // Anything left is a duplicate the index refused; it is now redundant.
    conn.execute("DELETE FROM node WHERE id = ?1", params![from])?;
    Ok(moved)
}

/// Move a tag to a position within its facet. Positions are dense and
/// zero-based afterwards, so a list drawn from `sort_order` never has gaps.
pub fn reorder(conn: &Connection, tag_id: &str, to: i64) -> Result<()> {
    require_tag(conn, tag_id)?;
    let facet: String = conn.query_row(
        "SELECT facet FROM tag WHERE node_id = ?1",
        params![tag_id],
        |r| r.get(0),
    )?;
    let mut q = conn.prepare(
        "SELECT node_id FROM tag WHERE facet = ?1 ORDER BY sort_order, node_id",
    )?;
    let mut order: Vec<String> = q
        .query_map(params![facet], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    drop(q);

    let Some(at) = order.iter().position(|x| x == tag_id) else {
        return Ok(());
    };
    let id = order.remove(at);
    let to = to.clamp(0, order.len() as i64) as usize;
    order.insert(to, id);

    for (i, node_id) in order.iter().enumerate() {
        conn.execute(
            "UPDATE tag SET sort_order = ?2 WHERE node_id = ?1",
            params![node_id, i as i64],
        )?;
    }
    Ok(())
}

/* ----------------------------------------------------------- promotion */

pub struct Promoted {
    pub collector_id: String,
    pub moved: usize,
}

/// Turn a tag into a Collector holding everything that carried it.
///
/// One-way on purpose. Tags describe; Collectors aggregate. A Collector that
/// could turn back into a tag would make the two interchangeable, and the
/// distinction is the only thing keeping "things that are like this" apart
/// from "things I have gathered". `promoted_from_tag_id` records where it came
/// from so a board that grew out of a tag can still say so — and is set to
/// NULL rather than cascading if the tag is later deleted.
pub fn promote_to_collector(
    conn: &Connection,
    tag_id: &str,
    name: Option<&str>,
    strip_tag: bool,
) -> Result<Promoted> {
    require_tag(conn, tag_id)?;
    let tag_name: String = conn.query_row(
        "SELECT display_name FROM node WHERE id = ?1",
        params![tag_id],
        |r| r.get(0),
    )?;
    let name = name.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&tag_name);

    let collector_id = uuid_v7();
    conn.execute(
        "INSERT INTO node(id, node_type, content_type, content_type_tree,
                          title, display_name, display_subtitle, icon_kind, source_kind)
         VALUES (?1, 'collector', ?2, ?3, ?4, ?4, 'collector', 'folder', 'app_generated')",
        params![
            collector_id,
            content_type::VIRTUAL,
            serde_json::to_string(&content_type::closure(content_type::VIRTUAL))?,
            name,
        ],
    )?;
    conn.execute(
        "INSERT INTO collector(node_id, collector_kind, promoted_from_tag_id)
         VALUES (?1, 'folder', ?2)",
        params![collector_id, tag_id],
    )?;

    let mut q = conn.prepare(
        "SELECT source_id FROM edge WHERE target_id = ?1 AND kind = 'tag_of'",
    )?;
    let carriers: Vec<String> = q
        .query_map(params![tag_id], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    drop(q);

    let mut moved = 0;
    for item in &carriers {
        // Again through the write path: dropping an item on a collector can
        // only become `contains`, and this file does not get to decide that.
        let linked = mutations::link(conn, item, "N", &collector_id, None, None)?;
        if !linked.existed {
            moved += 1;
        }
    }
    if strip_tag {
        unapply(conn, &carriers, tag_id)?;
    }
    Ok(Promoted {
        collector_id,
        moved,
    })
}

fn require_tag(conn: &Connection, id: &str) -> Result<()> {
    let ok: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tag WHERE node_id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if ok == 0 {
        return Err(anyhow!("not a tag: {id}"));
    }
    Ok(())
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

    fn item(c: &Connection, id: &str) -> String {
        c.execute(
            "INSERT INTO node(id, node_type, content_type, display_name, locator)
             VALUES (?1, 'media', 'public.jpeg', ?1, ?1)",
            params![id],
        )
        .unwrap();
        id.to_string()
    }

    #[test]
    fn the_same_word_can_be_two_facets_but_not_two_tags_in_one() {
        let c = db();
        let a = ensure(&c, "Coastline", "environment").unwrap();
        let b = ensure(&c, "coastline", "subject").unwrap();
        assert_ne!(a, b, "one word, two different claims");
        let again = ensure(&c, "  coastline  ", "environment").unwrap();
        assert_eq!(a, again, "same facet, same word — the same tag");
        assert_eq!(list(&c).unwrap().len(), 2);
    }

    #[test]
    fn a_tag_lands_in_the_tier_its_facet_belongs_to() {
        let c = db();
        let id = ensure(&c, "1970s", "era").unwrap();
        let t = list(&c).unwrap().into_iter().find(|t| t.id == id).unwrap();
        assert_eq!(t.tier, 1);
        assert!(ensure(&c, "x", "mood").is_err());
    }

    #[test]
    fn applying_is_a_batch_and_reports_only_what_changed() {
        let c = db();
        let items: Vec<String> = ["a", "b", "c"].iter().map(|i| item(&c, i)).collect();
        let tag = ensure(&c, "coast", "environment").unwrap();
        assert_eq!(apply(&c, &items, &tag).unwrap(), 3);
        // Applying it again is not an error, and changes nothing.
        assert_eq!(apply(&c, &items, &tag).unwrap(), 0);
        assert_eq!(of_node(&c, "a").unwrap().len(), 1);
        assert_eq!(list(&c).unwrap()[0].usage, 3);
    }

    #[test]
    fn removing_reports_only_the_items_that_carried_it() {
        let c = db();
        let all: Vec<String> = ["a", "b"].iter().map(|i| item(&c, i)).collect();
        let tag = ensure(&c, "coast", "environment").unwrap();
        apply(&c, &all[..1], &tag).unwrap();
        assert_eq!(unapply(&c, &all, &tag).unwrap(), 1);
        assert!(of_node(&c, "a").unwrap().is_empty());
    }

    #[test]
    fn renaming_into_an_existing_name_is_refused_with_the_remedy() {
        let c = db();
        ensure(&c, "coast", "environment").unwrap();
        let other = ensure(&c, "shore", "environment").unwrap();
        let err = rename(&c, &other, "Coast").unwrap_err().to_string();
        assert!(err.contains("merge"), "{err}");
        // A different facet is a different claim, so it is allowed.
        let subject = ensure(&c, "shore", "subject").unwrap();
        rename(&c, &subject, "Coast").unwrap();
    }

    #[test]
    fn deleting_a_tag_leaves_the_items_that_carried_it() {
        let c = db();
        let items = vec![item(&c, "a")];
        let tag = ensure(&c, "coast", "environment").unwrap();
        apply(&c, &items, &tag).unwrap();
        assert_eq!(delete(&c, &tag).unwrap(), 1);
        let left: i64 = c
            .query_row("SELECT COUNT(*) FROM node WHERE node_type='media'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(left, 1);
        assert!(of_node(&c, "a").unwrap().is_empty());
    }

    #[test]
    fn merging_survives_an_item_that_carries_both_tags() {
        // The case that breaks a naive UPDATE: 'b' has both, so one of its two
        // rows cannot move without colliding with the other.
        let c = db();
        let a = item(&c, "a");
        let b = item(&c, "b");
        let from = ensure(&c, "seaside", "environment").unwrap();
        let into = ensure(&c, "coast", "environment").unwrap();
        apply(&c, &[a.clone(), b.clone()], &from).unwrap();
        apply(&c, &[b.clone()], &into).unwrap();

        merge(&c, &from, &into).unwrap();
        assert_eq!(of_node(&c, "a").unwrap().len(), 1);
        assert_eq!(of_node(&c, "b").unwrap().len(), 1, "no duplicate left behind");
        assert_eq!(list(&c).unwrap().len(), 1);
        assert_eq!(list(&c).unwrap()[0].usage, 2);
    }

    #[test]
    fn reordering_leaves_positions_dense() {
        let c = db();
        let a = ensure(&c, "a", "subject").unwrap();
        ensure(&c, "b", "subject").unwrap();
        ensure(&c, "c", "subject").unwrap();
        reorder(&c, &a, 2).unwrap();
        let names: Vec<String> = list(&c).unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["b", "c", "a"]);
        let orders: Vec<i64> = list(&c).unwrap().into_iter().map(|t| t.sort_order).collect();
        assert_eq!(orders, vec![0, 1, 2]);
    }

    #[test]
    fn reordering_past_the_end_lands_at_the_end_rather_than_failing() {
        let c = db();
        let a = ensure(&c, "a", "subject").unwrap();
        ensure(&c, "b", "subject").unwrap();
        reorder(&c, &a, 99).unwrap();
        let names: Vec<String> = list(&c).unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["b", "a"]);
    }

    #[test]
    fn promotion_gathers_the_items_and_records_where_it_came_from() {
        let c = db();
        let items: Vec<String> = ["a", "b"].iter().map(|i| item(&c, i)).collect();
        let tag = ensure(&c, "bergamo", "subject").unwrap();
        apply(&c, &items, &tag).unwrap();

        let p = promote_to_collector(&c, &tag, Some("Bergamo 2024"), false).unwrap();
        assert_eq!(p.moved, 2);
        let (name, from): (String, Option<String>) = c
            .query_row(
                "SELECT n.display_name, c.promoted_from_tag_id
                   FROM node n JOIN collector c ON c.node_id = n.id WHERE n.id = ?1",
                params![p.collector_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "Bergamo 2024");
        assert_eq!(from.as_deref(), Some(tag.as_str()));
        // Not stripped, so the tag is still carried.
        assert_eq!(of_node(&c, "a").unwrap().len(), 1);
    }

    #[test]
    fn promotion_can_strip_the_tag_it_came_from() {
        let c = db();
        let items: Vec<String> = ["a"].iter().map(|i| item(&c, i)).collect();
        let tag = ensure(&c, "bergamo", "subject").unwrap();
        apply(&c, &items, &tag).unwrap();
        promote_to_collector(&c, &tag, None, true).unwrap();
        assert!(of_node(&c, "a").unwrap().is_empty());
        // The tag itself survives — stripping removes the application, not the
        // vocabulary.
        assert_eq!(list(&c).unwrap().len(), 1);
    }

    #[test]
    fn everything_that_manages_a_tag_refuses_a_node_that_is_not_one() {
        let c = db();
        item(&c, "a");
        assert!(rename(&c, "a", "x").is_err());
        assert!(delete(&c, "a").is_err());
        assert!(set_facet(&c, "a", "era").is_err());
        assert!(apply(&c, &["a".into()], "a").is_err());
        assert!(promote_to_collector(&c, "a", None, false).is_err());
    }
}
