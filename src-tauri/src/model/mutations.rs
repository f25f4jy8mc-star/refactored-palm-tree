//! The write path (G24).
//!
//! Phase 0 defined nine read models and no mutations, because every view up to
//! Compass only read. Compass creates and destroys edges by drag, so this is
//! where that gets named.
//!
//! Five operations, and each is one statement's worth of intent:
//!
//!   link           create an edge in a compass direction
//!   unlink         remove one
//!   reorder        move one within its slot
//!   set_label      name a relationship
//!   resolve        accept or dismiss a suggestion
//!
//! Every one takes a transaction and returns what changed, so the caller bumps
//! the revision and emits one change event — invariant 10, one writer per
//! invalidation.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};

use super::projections::{compass_of, kind_for_drop};
use super::scan::uuid_v7;

fn node_type(conn: &Connection, id: &str) -> Result<String> {
    conn.query_row("SELECT node_type FROM node WHERE id = ?1", params![id], |r| {
        r.get(0)
    })
    .map_err(|_| anyhow!("no such node: {id}"))
}

/// Which kind a drop produces (G21), with one refinement the read rule cannot
/// express on its own: **a tag dropped on a tag's North is a broader tag, not a
/// tagging.** Tags cannot be tagged — that is what the `tag` capability says —
/// so tag-to-tag relationships are compass links.
fn kind_for(conn: &Connection, source: &str, compass: &str, target: &str) -> Result<&'static str> {
    let target_type = node_type(conn, target)?;
    if compass == "N" && target_type == "tag" && node_type(conn, source)? == "tag" {
        return Ok("compass_n");
    }
    Ok(kind_for_drop(compass, &target_type))
}

/// Whether a direction reads the same from both ends. West and East do; North
/// and South are converse (G23). It decides how a duplicate is detected.
fn symmetric(compass: &str) -> bool {
    matches!(compass, "W" | "E")
}

pub struct Linked {
    pub edge_id: String,
    /// True when the edge already existed and nothing was written. Callers use
    /// it to skip the change event.
    pub existed: bool,
}

/// Create an edge from `source` in `compass`, pointing at `target`.
///
/// Idempotent, and refuses three things outright rather than storing something
/// that reads back as nonsense.
pub fn link(
    conn: &Connection,
    source: &str,
    compass: &str,
    target: &str,
    label: Option<&str>,
    scope: Option<&str>,
) -> Result<Linked> {
    if source == target {
        return Err(anyhow!("an item cannot be linked to itself"));
    }
    if !matches!(compass, "N" | "S" | "W" | "E") {
        return Err(anyhow!("not a compass direction: {compass}"));
    }
    let kind = kind_for(conn, source, compass, target)?;

    // Already there, written from this side.
    if let Some(id) = existing(conn, source, target, kind, scope)? {
        return Ok(Linked { edge_id: id, existed: true });
    }

    // Already there, written from the other side. Only lateral directions can
    // be duplicated this way, because only they read the same from both ends —
    // A→B West and B→A West are one claim, and storing both is what left the
    // backfill with pairs to report.
    if symmetric(compass) {
        if let Some(id) = existing(conn, target, source, kind, scope)? {
            return Ok(Linked { edge_id: id, existed: true });
        }
    }

    let ordinal: i64 = conn.query_row(
        "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM edge WHERE source_id = ?1 AND kind = ?2",
        params![source, kind],
        |r| r.get(0),
    )?;

    let edge_id = uuid_v7();
    conn.execute(
        "INSERT INTO edge(id, source_id, target_id, kind, label, ordinal,
                          scope_collector_id, status, origin)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'declared', 'user')",
        params![edge_id, source, target, kind, label, ordinal, scope],
    )?;
    Ok(Linked { edge_id, existed: false })
}

/// Two queries rather than one, because the placeholder counts differ.
///
/// `scope_collector_id IS NULL` cannot be written as `= ?4` — SQLite compares
/// NULL to everything as unknown, so an equality test never matches a global
/// edge. That is the same nullable-column trap that migration 002 of the old
/// schema existed to repair, and it forces the branch. Building only the SQL
/// while passing one parameter list is what broke this the first time.
fn existing(
    conn: &Connection,
    source: &str,
    target: &str,
    kind: &str,
    scope: Option<&str>,
) -> Result<Option<String>> {
    let found = match scope {
        Some(s) => conn
            .query_row(
                "SELECT id FROM edge WHERE source_id = ?1 AND target_id = ?2 AND kind = ?3
                   AND scope_collector_id = ?4",
                params![source, target, kind, s],
                |r| r.get(0),
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT id FROM edge WHERE source_id = ?1 AND target_id = ?2 AND kind = ?3
                   AND scope_collector_id IS NULL",
                params![source, target, kind],
                |r| r.get(0),
            )
            .optional()?,
    };
    Ok(found)
}

/// Remove one edge, and close the gap it leaves in its slot so ordinals stay
/// contiguous. A slot with holes still renders correctly, but reordering into
/// a hole would not.
pub fn unlink(conn: &Connection, edge_id: &str) -> Result<()> {
    let (source, kind): (String, String) = conn
        .query_row(
            "SELECT source_id, kind FROM edge WHERE id = ?1",
            params![edge_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| anyhow!("no such edge: {edge_id}"))?;
    conn.execute("DELETE FROM edge WHERE id = ?1", params![edge_id])?;
    renumber(conn, &source, &kind)?;
    Ok(())
}

/// Move an edge to a position within its own `(source, kind)` group.
///
/// Within the kind, not the whole slot: a slot is ordered by target type first
/// (G25), so dragging across a type boundary would need a slot-level ordinal
/// and a schema change. Tags stay above collectors whatever you drag.
pub fn reorder(conn: &Connection, edge_id: &str, to: i64) -> Result<()> {
    let (source, kind): (String, String) = conn
        .query_row(
            "SELECT source_id, kind FROM edge WHERE id = ?1",
            params![edge_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| anyhow!("no such edge: {edge_id}"))?;

    let mut q = conn.prepare(
        "SELECT id FROM edge WHERE source_id = ?1 AND kind = ?2 ORDER BY ordinal, id",
    )?;
    let mut ids: Vec<String> = q
        .query_map(params![source, kind], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    drop(q);

    let Some(from) = ids.iter().position(|i| i == edge_id) else {
        return Ok(());
    };
    let to = to.clamp(0, ids.len() as i64 - 1) as usize;
    let moved = ids.remove(from);
    ids.insert(to, moved);

    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE edge SET ordinal = ?2 WHERE id = ?1",
            params![id, i as i64],
        )?;
    }
    Ok(())
}

fn renumber(conn: &Connection, source: &str, kind: &str) -> Result<()> {
    let mut q = conn.prepare(
        "SELECT id FROM edge WHERE source_id = ?1 AND kind = ?2 ORDER BY ordinal, id",
    )?;
    let ids: Vec<String> = q
        .query_map(params![source, kind], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    drop(q);
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE edge SET ordinal = ?2 WHERE id = ?1",
            params![id, i as i64],
        )?;
    }
    Ok(())
}

pub fn set_label(conn: &Connection, edge_id: &str, label: Option<&str>) -> Result<()> {
    let n = conn.execute(
        "UPDATE edge SET label = ?2 WHERE id = ?1",
        params![edge_id, label.filter(|l| !l.trim().is_empty())],
    )?;
    if n == 0 {
        return Err(anyhow!("no such edge: {edge_id}"));
    }
    Ok(())
}

/// Accepting a suggestion promotes it to declared but **leaves `origin`
/// alone**. That is deliberate: origin is what makes machine output revertible
/// in bulk, and rewriting it to 'user' on acceptance would erase the only
/// record that the software proposed it.
pub fn accept(conn: &Connection, edge_id: &str) -> Result<()> {
    let n = conn.execute(
        "UPDATE edge SET status = 'declared' WHERE id = ?1 AND status = 'suggested'",
        params![edge_id],
    )?;
    if n == 0 {
        return Err(anyhow!("no suggestion with id {edge_id}"));
    }
    Ok(())
}

/// Dismissing removes the edge and remembers the pair, so the same suggestion
/// does not come back on the next pass.
pub fn dismiss(conn: &Connection, edge_id: &str) -> Result<()> {
    let (source, target, kind): (String, Option<String>, String) = conn
        .query_row(
            "SELECT source_id, target_id, kind FROM edge WHERE id = ?1 AND status = 'suggested'",
            params![edge_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| anyhow!("no suggestion with id {edge_id}"))?;

    let key = match &target {
        Some(t) => format!("{kind}:{source}:{t}"),
        None => format!("{kind}:{source}"),
    };
    conn.execute(
        "INSERT OR IGNORE INTO dismissed(dismiss_key, kind) VALUES (?1, ?2)",
        params![key, kind],
    )?;
    conn.execute("DELETE FROM edge WHERE id = ?1", params![edge_id])?;
    Ok(())
}

/// Every machine-created edge of one origin, gone. The safety valve that makes
/// suggestion features acceptable in the first place.
pub fn revert_origin(conn: &Connection, origin: &str) -> Result<usize> {
    if origin == "user" {
        return Err(anyhow!("refusing to bulk-delete what the user asserted"));
    }
    Ok(conn.execute("DELETE FROM edge WHERE origin = ?1", params![origin])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        for (id, nt, ct) in [
            ("a", "media", "public.jpeg"),
            ("b", "media", "public.jpeg"),
            ("c", "media", "public.jpeg"),
            ("t1", "tag", "app.archiva.tag"),
            ("t2", "tag", "app.archiva.tag"),
            ("k", "collector", "app.archiva.collector.folder"),
        ] {
            c.execute(
                "INSERT INTO node(id, node_type, content_type, display_name) VALUES (?1,?2,?3,?1)",
                params![id, nt, ct],
            )
            .unwrap();
        }
        c.execute("INSERT INTO tag VALUES('t1','subject',3,0)", []).unwrap();
        c.execute("INSERT INTO tag VALUES('t2','subject',3,1)", []).unwrap();
        c.execute(
            "INSERT INTO collector(node_id, collector_kind) VALUES('k','folder')",
            [],
        )
        .unwrap();
        c
    }

    fn kind_of(c: &Connection, id: &str) -> String {
        c.query_row("SELECT kind FROM edge WHERE id = ?1", params![id], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn the_target_decides_the_kind() {
        let c = db();
        assert_eq!(kind_of(&c, &link(&c, "a", "N", "t1", None, None).unwrap().edge_id), "tag_of");
        assert_eq!(kind_of(&c, &link(&c, "a", "N", "k", None, None).unwrap().edge_id), "contains");
        assert_eq!(kind_of(&c, &link(&c, "a", "N", "b", None, None).unwrap().edge_id), "compass_n");
        assert_eq!(kind_of(&c, &link(&c, "a", "E", "c", None, None).unwrap().edge_id), "compass_e");
    }

    /// Tags cannot be tagged, so a tag on a tag's North is a broader tag.
    #[test]
    fn a_tag_dropped_on_a_tag_is_a_broader_tag_not_a_tagging() {
        let c = db();
        let e = link(&c, "t1", "N", "t2", None, None).unwrap();
        assert_eq!(kind_of(&c, &e.edge_id), "compass_n");
    }

    #[test]
    fn linking_twice_is_the_same_link() {
        let c = db();
        let first = link(&c, "a", "N", "t1", None, None).unwrap();
        let second = link(&c, "a", "N", "t1", None, None).unwrap();
        assert!(!first.existed && second.existed);
        assert_eq!(first.edge_id, second.edge_id);
    }

    /// G23 again, from the write side. West is symmetric, so asserting it from
    /// the far end is the same claim — storing both is what left the backfill
    /// with redundant pairs to report.
    #[test]
    fn asserting_a_lateral_link_from_either_end_makes_one_edge() {
        let c = db();
        let there = link(&c, "a", "W", "b", None, None).unwrap();
        let back = link(&c, "b", "W", "a", None, None).unwrap();
        assert!(back.existed);
        assert_eq!(there.edge_id, back.edge_id);
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM edge WHERE kind = 'compass_w'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    /// North and South are converse, not symmetric, so both are real claims.
    #[test]
    fn a_vertical_link_asserted_both_ways_makes_two_edges() {
        let c = db();
        link(&c, "a", "N", "b", None, None).unwrap();
        let back = link(&c, "b", "N", "a", None, None).unwrap();
        assert!(!back.existed);
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM edge WHERE kind = 'compass_n'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn an_item_cannot_be_linked_to_itself() {
        let c = db();
        assert!(link(&c, "a", "W", "a", None, None).is_err());
    }

    #[test]
    fn ordinals_stay_contiguous_after_a_removal() {
        let c = db();
        let e1 = link(&c, "a", "S", "b", None, None).unwrap().edge_id;
        let e2 = link(&c, "a", "S", "c", None, None).unwrap().edge_id;
        unlink(&c, &e1).unwrap();
        let o: i64 = c
            .query_row("SELECT ordinal FROM edge WHERE id = ?1", params![e2], |r| r.get(0))
            .unwrap();
        assert_eq!(o, 0, "the survivor closed the gap");
    }

    #[test]
    fn reorder_moves_within_the_kind_and_renumbers() {
        let c = db();
        let ids: Vec<String> = ["b", "c", "t1"]
            .iter()
            .enumerate()
            .map(|(_, t)| {
                let compass = if *t == "t1" { "S" } else { "S" };
                link(&c, "a", compass, t, None, None).unwrap().edge_id
            })
            .collect();
        // t1 as a South target is a plain compass_s, so all three share a kind.
        reorder(&c, &ids[2], 0).unwrap();
        let order: Vec<String> = {
            let mut q = c
                .prepare("SELECT id FROM edge WHERE source_id='a' AND kind='compass_s' ORDER BY ordinal")
                .unwrap();
            q.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
        };
        assert_eq!(order[0], ids[2]);
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn accepting_a_suggestion_keeps_its_origin() {
        let c = db();
        c.execute(
            "INSERT INTO edge(id, source_id, target_id, kind, status, origin)
             VALUES ('s1','a','b','compass_e','suggested','cooccurrence')",
            [],
        )
        .unwrap();
        accept(&c, "s1").unwrap();
        let (status, origin): (String, String) = c
            .query_row("SELECT status, origin FROM edge WHERE id='s1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(status, "declared");
        assert_eq!(origin, "cooccurrence", "origin is what makes it revertible");
    }

    #[test]
    fn dismissing_removes_it_and_remembers_the_pair() {
        let c = db();
        c.execute(
            "INSERT INTO edge(id, source_id, target_id, kind, status, origin)
             VALUES ('s1','a','b','compass_e','suggested','cooccurrence')",
            [],
        )
        .unwrap();
        dismiss(&c, "s1").unwrap();
        let edges: i64 = c
            .query_row("SELECT COUNT(*) FROM edge WHERE id='s1'", [], |r| r.get(0))
            .unwrap();
        let dismissals: i64 = c
            .query_row("SELECT COUNT(*) FROM dismissed", [], |r| r.get(0))
            .unwrap();
        assert_eq!((edges, dismissals), (0, 1));
    }

    #[test]
    fn machine_output_can_be_reverted_in_bulk_but_yours_cannot() {
        let c = db();
        link(&c, "a", "W", "b", None, None).unwrap();
        c.execute(
            "INSERT INTO edge(id, source_id, target_id, kind, status, origin)
             VALUES ('s1','a','c','compass_e','suggested','cooccurrence')",
            [],
        )
        .unwrap();
        assert_eq!(revert_origin(&c, "cooccurrence").unwrap(), 1);
        assert!(revert_origin(&c, "user").is_err());
        let left: i64 = c
            .query_row("SELECT COUNT(*) FROM edge", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1, "what you asserted survives");
    }

    #[test]
    fn compass_and_kind_agree_for_everything_link_can_create() {
        let c = db();
        for (compass, target) in [("N", "t1"), ("N", "k"), ("N", "b"), ("S", "b"), ("W", "b"), ("E", "c")] {
            let e = link(&c, "a", compass, target, None, None).unwrap();
            assert_eq!(
                compass_of(&kind_of(&c, &e.edge_id)),
                Some(compass),
                "{compass} onto {target} read back wrong"
            );
        }
    }
}
