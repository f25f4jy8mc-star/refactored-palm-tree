//! `p_tree` — Miller's cascade, as a path rather than a stack of caches.
//!
//! Each column is the previous column's selected row, expanded. This is
//! deliberately **not** `p_rows` reused: `p_rows`' unscoped listing is the
//! whole flat library (that is exactly what Library itself wants), while a
//! Miller root column wants only the nodes nothing else `contains` — real
//! hierarchy, or every column would show the same flat pile Library already
//! does and the view would carry no information Library doesn't. A scoped
//! column (a folder's children) is genuinely shared shape, so both cases
//! are one small query here, built into rows with `projections::row` — the
//! same capability-resolved shape `p_detail` and `p_search` already return,
//! so this is a fourth *caller* of that logic, not a fourth copy of it.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::projections::{self, Row};

#[derive(Debug, Serialize)]
pub struct Column {
    /// `None` for the library root; a collector id for every column after.
    pub scope_id: Option<String>,
    pub title: String,
    pub rows: Vec<Row>,
}

/// Ids and names of a column's members, sorted the same way `p_rows`' name
/// sort is (case-insensitive, tie-broken by id so ties are stable across
/// calls) — `scope = None` is "nothing contains this", not "everything".
fn children_of(conn: &Connection, scope: Option<&str>) -> Result<Vec<(String, String)>> {
    let sql = match scope {
        Some(_) => {
            "SELECT n.id, n.display_name FROM node n
               JOIN edge e ON e.source_id = n.id AND e.kind = 'contains' AND e.target_id = ?1"
        }
        None => {
            // `contains` runs item -> collector (source is the contained
            // item, target is the collector), matching the compass table in
            // §1.7 — "contains (item -> collector)". A root member is a node
            // that is nobody's *source* here, not nobody's target.
            "SELECT n.id, n.display_name FROM node n
              WHERE n.node_type <> 'tag' AND ?1 IS NULL
                AND NOT EXISTS (
                  SELECT 1 FROM edge e WHERE e.kind = 'contains' AND e.source_id = n.id
                )"
        }
    };
    let mut q = conn.prepare(sql)?;
    let mut out: Vec<(String, String)> = q
        .query_map(params![scope], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(&b.0)));
    Ok(out)
}

fn column_for(conn: &Connection, scope: Option<&str>, title: &str) -> Result<Column> {
    let members = children_of(conn, scope)?;
    let mut rows = Vec::with_capacity(members.len());
    for (id, _) in members {
        rows.push(projections::row(conn, &id)?);
    }
    Ok(Column {
        scope_id: scope.map(str::to_string),
        title: title.to_string(),
        rows,
    })
}

/// Walk `path`, one column per id. The walk stops the moment an id doesn't
/// name a row in the previous column, or names one that isn't a collector
/// (only collectors expand — §2.3, `expand` is granted at
/// `app.archiva.collector`) — the caller asked to descend into something
/// that no longer describes a valid drill-down, and the honest answer is
/// the columns that are still real, not an error or a guess.
pub fn tree(conn: &Connection, path: &[String]) -> Result<Vec<Column>> {
    let mut columns = vec![column_for(conn, None, "Library")?];
    for id in path {
        let Some(last) = columns.last() else { break };
        let Some(row) = last.rows.iter().find(|r| &r.id == id) else {
            break;
        };
        if row.node_type != "collector" {
            break;
        }
        let title = row.display_name.clone();
        columns.push(column_for(conn, Some(id.as_str()), &title)?);
    }
    Ok(columns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        let mk_media = |id: &str, name: &str| {
            c.execute(
                "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,icon_kind)
                 VALUES (?1,'media','public.jpeg','[]',?2,'image')",
                params![id, name],
            )
            .unwrap();
        };
        let mk_folder = |id: &str, name: &str| {
            c.execute(
                "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,icon_kind)
                 VALUES (?1,'collector','app.archiva.collector.folder','[]',?2,'folder')",
                params![id, name],
            )
            .unwrap();
            c.execute(
                "INSERT INTO collector(node_id,collector_kind) VALUES (?1,'folder')",
                params![id],
            )
            .unwrap();
        };
        let contains = |parent: &str, child: &str, edge_id: &str| {
            c.execute(
                "INSERT INTO edge(id,source_id,target_id,kind) VALUES (?1,?2,?3,'contains')",
                params![edge_id, child, parent],
            )
            .unwrap();
        };

        mk_folder("root-folder", "Trips");
        mk_media("top-photo", "Cover");
        mk_folder("sub-folder", "Bergamo");
        mk_media("nested-photo", "Arcade");
        mk_media("uncontained-photo", "Loose");

        contains("root-folder", "top-photo", "e1"); // Trips/Cover — a leaf
        contains("root-folder", "sub-folder", "e2"); // Trips/Bergamo — a folder
        contains("sub-folder", "nested-photo", "e3"); // Trips/Bergamo/Arcade
        // "Loose" is contained by nothing, so it too is a root member.
        c
    }

    #[test]
    fn the_root_column_holds_only_uncontained_nodes() {
        let c = seed();
        let cols = tree(&c, &[]).unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].scope_id, None);
        assert_eq!(cols[0].title, "Library");
        let names: Vec<&str> = cols[0].rows.iter().map(|r| r.display_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Loose", "Trips"],
            "Cover, Bergamo and Arcade are all contained by something and must not appear at the root"
        );
    }

    #[test]
    fn descending_into_a_folder_adds_a_column_of_its_children() {
        let c = seed();
        let cols = tree(&c, &["root-folder".to_string()]).unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[1].scope_id.as_deref(), Some("root-folder"));
        assert_eq!(cols[1].title, "Trips");
        let names: Vec<&str> = cols[1].rows.iter().map(|r| r.display_name.as_str()).collect();
        assert_eq!(names, vec!["Bergamo", "Cover"], "alphabetical, case-insensitive");
    }

    #[test]
    fn the_cascade_walks_arbitrarily_deep() {
        let c = seed();
        let cols = tree(&c, &["root-folder".to_string(), "sub-folder".to_string()]).unwrap();
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[2].title, "Bergamo");
        assert_eq!(cols[2].rows[0].id, "nested-photo");
    }

    #[test]
    fn selecting_a_leaf_does_not_expand_a_further_column() {
        let c = seed();
        // "top-photo" is media, not a collector — expand is not granted, but
        // it must still be reachable as a root-level id for this check to
        // mean anything, so route through a folder first.
        let cols = tree(&c, &["root-folder".to_string(), "top-photo".to_string()]).unwrap();
        assert_eq!(cols.len(), 2, "a non-collector must not produce a further column");
    }

    #[test]
    fn a_stale_id_stops_the_cascade_rather_than_erroring() {
        let c = seed();
        let cols = tree(&c, &["does-not-exist".to_string()]).unwrap();
        assert_eq!(cols.len(), 1);
    }

    #[test]
    fn a_valid_id_after_a_stale_one_is_never_reached() {
        let c = seed();
        let cols = tree(
            &c,
            &["does-not-exist".to_string(), "root-folder".to_string()],
        )
        .unwrap();
        assert_eq!(cols.len(), 1, "the walk stops at the first break, it does not skip ahead");
    }

    #[test]
    fn a_column_row_carries_resolved_capabilities_not_just_a_bare_id() {
        let c = seed();
        let cols = tree(&c, &["root-folder".to_string()]).unwrap();
        let folder = cols[1].rows.iter().find(|r| r.id == "sub-folder").unwrap();
        assert!(folder.capabilities.iter().any(|cap| cap == "expand"));
    }
}
