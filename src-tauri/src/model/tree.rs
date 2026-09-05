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

/// Where a cascade's first column starts when there is no scope.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Root {
    /// Everything nothing contains, the watched folders among it. The
    /// Library's Hierarchy, where the disk scaffolding is the point — it is
    /// what tells you where a thing actually lives.
    Library,
    /// The same, with every watched folder replaced by what is inside it. The
    /// Viewer is a workspace rather than a picture of the disk, and its flat
    /// modes already leave the folder scaffolding out (`rowtree::source`), so
    /// its cascade starting at a list of mount points was the odd one out.
    /// Hoisting rather than hiding: dropping them outright would put every
    /// indexed file behind a folder the pane refuses to draw.
    Workspace,
}

/// The watched roots: a folder Collector this build derived from disk that
/// nothing else contains. Computed rather than read off `display_subtitle`,
/// which is written once at creation and cannot know that a source was
/// removed afterwards.
const WATCHED_ROOTS: &str = "SELECT n.id FROM node n
     WHERE n.node_type = 'collector' AND n.source_kind = 'app_generated'
       AND n.locator IS NOT NULL
       AND NOT EXISTS (
         SELECT 1 FROM edge e WHERE e.kind = 'contains' AND e.source_id = n.id
       )";

/// Ids and names of a column's members, sorted the same way `p_rows`' name
/// sort is (case-insensitive, tie-broken by id so ties are stable across
/// calls) — `scope = None` is "nothing contains this", not "everything".
fn children_of(conn: &Connection, scope: Option<&str>, root: Root) -> Result<Vec<(String, String)>> {
    let scoped = "SELECT n.id, n.display_name FROM node n
           JOIN edge e ON e.source_id = n.id AND e.kind = 'contains' AND e.target_id = ?1";
    // `contains` runs item -> collector (source is the contained item, target
    // is the collector), matching the compass table in §1.7 — "contains
    // (item -> collector)". A root member is a node that is nobody's *source*
    // here, not nobody's target.
    let library = "SELECT n.id, n.display_name FROM node n
          WHERE n.node_type <> 'tag' AND ?1 IS NULL
            AND NOT EXISTS (
              SELECT 1 FROM edge e WHERE e.kind = 'contains' AND e.source_id = n.id
            )"
    .to_string();
    // Held by nothing except a watched root: uncontained items and gatherings
    // as before, plus whatever sits at the top of each watched folder.
    let workspace = format!(
        "WITH watched AS ({WATCHED_ROOTS})
         SELECT n.id, n.display_name FROM node n
          WHERE n.node_type <> 'tag' AND ?1 IS NULL
            AND n.id NOT IN (SELECT id FROM watched)
            AND NOT EXISTS (
              SELECT 1 FROM edge e
               WHERE e.kind = 'contains' AND e.source_id = n.id
                 AND e.target_id NOT IN (SELECT id FROM watched)
            )"
    );
    let sql = match (scope, root) {
        (Some(_), _) => scoped.to_string(),
        (None, Root::Library) => library,
        (None, Root::Workspace) => workspace,
    };
    let mut q = conn.prepare(&sql)?;
    let mut out: Vec<(String, String)> = q
        .query_map(params![scope], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(&b.0)));
    Ok(out)
}

fn column_for(conn: &Connection, scope: Option<&str>, title: &str, root: Root) -> Result<Column> {
    let members = children_of(conn, scope, root)?;
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

/// The cascade, starting anywhere.
///
/// `root` is the collector the first column shows the inside of; `None` is
/// the library root — the nodes nothing contains. Starting anywhere is what
/// a Viewer scoped to a folder needs: a folder nested inside another is not
/// in the library's root column, so walking a path that began with it broke
/// on the first step and returned the root column alone. The pane then
/// dropped that column as "the root it did not ask for" and drew nothing.
///
/// Walk `path`, one column per id. The walk stops the moment an id doesn't
/// name a row in the previous column, or names one that isn't a collector
/// (only collectors expand — §2.3, `expand` is granted at
/// `app.archiva.collector`) — the caller asked to descend into something
/// that no longer describes a valid drill-down, and the honest answer is
/// the columns that are still real, not an error or a guess.
pub fn tree_from(conn: &Connection, root: Option<&str>, path: &[String]) -> Result<Vec<Column>> {
    cascade(conn, root, path, Root::Library)
}

/// The cascade as the Viewer wants it: the watched folders themselves are not
/// drawn, their contents are. Only the first column can differ — once you are
/// inside a folder, its subfolders are its contents and hiding them there
/// would lose everything beside the files.
pub fn workspace(conn: &Connection, root: Option<&str>, path: &[String]) -> Result<Vec<Column>> {
    cascade(conn, root, path, Root::Workspace)
}

fn cascade(
    conn: &Connection,
    root: Option<&str>,
    path: &[String],
    start: Root,
) -> Result<Vec<Column>> {
    let title = match root {
        None => "Library".to_string(),
        Some(id) => conn
            .query_row(
                "SELECT display_name FROM node WHERE id = ?1",
                params![id],
                |r| r.get::<_, String>(0),
            )
            // A scope that no longer exists is not an error: the columns that
            // are still real is the honest answer here too.
            .unwrap_or_else(|_| "Library".to_string()),
    };
    let mut columns = vec![column_for(conn, root, &title, start)?];
    for id in path {
        let Some(last) = columns.last() else { break };
        let Some(row) = last.rows.iter().find(|r| &r.id == id) else {
            break;
        };
        if row.node_type != "collector" {
            break;
        }
        let title = row.display_name.clone();
        // Every column after the first is the inside of a collector, which is
        // the same listing whichever root the cascade started from.
        columns.push(column_for(conn, Some(id.as_str()), &title, Root::Library)?);
    }
    Ok(columns)
}

/// The cascade from the library root.
pub fn tree(conn: &Connection, path: &[String]) -> Result<Vec<Column>> {
    tree_from(conn, None, path)
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

    /// A watched root as `folders::rebuild` makes one: derived from disk, so
    /// `app_generated` with a locator, and contained by nothing.
    fn seed_watched() -> Connection {
        let c = seed();
        c.execute(
            "UPDATE node SET source_kind='app_generated', locator='/photos'
              WHERE id='root-folder'",
            [],
        )
        .unwrap();
        c.execute(
            "UPDATE node SET source_kind='app_generated', locator='/photos/bergamo'
              WHERE id='sub-folder'",
            [],
        )
        .unwrap();
        c
    }

    fn names(col: &Column) -> Vec<&str> {
        col.rows.iter().map(|r| r.display_name.as_str()).collect()
    }

    #[test]
    fn the_viewer_starts_inside_the_watched_folders_rather_than_at_them() {
        // Reported: the Viewer shouldn't display the watched folders. Its flat
        // modes already leave them out, so its cascade was the odd one out.
        let c = seed_watched();
        let cols = workspace(&c, None, &[]).unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(
            names(&cols[0]),
            vec!["Bergamo", "Cover", "Loose"],
            "the contents of Trips, plus what was never in a watched folder"
        );
        assert!(
            !names(&cols[0]).contains(&"Trips"),
            "the watched folder itself is not drawn"
        );
    }

    #[test]
    fn hoisting_keeps_everything_reachable() {
        // Hiding the watched roots outright would put every indexed file
        // behind a folder the pane refuses to draw.
        let c = seed_watched();
        let cols = workspace(&c, None, &["sub-folder".to_string()]).unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(names(&cols[1]), vec!["Arcade"]);
    }

    #[test]
    fn the_library_cascade_still_shows_the_watched_folders() {
        // Hierarchy is where the disk scaffolding is the point: it is what
        // tells you where a thing actually lives.
        let c = seed_watched();
        let cols = tree(&c, &[]).unwrap();
        assert_eq!(names(&cols[0]), vec!["Loose", "Trips"]);
    }

    #[test]
    fn a_subfolder_is_only_hidden_at_the_top() {
        // Bergamo is a folder too, and inside Trips it is content.
        let c = seed_watched();
        let cols = workspace(&c, Some("root-folder"), &[]).unwrap();
        assert_eq!(names(&cols[0]), vec!["Bergamo", "Cover"]);
    }

    #[test]
    fn something_held_by_a_gathering_as_well_stays_where_it_was_put() {
        // Only "held by nothing but a watched root" is hoisted: a photograph
        // you also filed in a board of your own belongs to that board, and
        // showing it at the top as well would be the duplication the tree
        // rebuild exists to remove.
        let c = seed_watched();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,icon_kind,
                              source_kind)
             VALUES ('board','collector','app.archiva.collector','[]','My board','folder',
                     'app_generated')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id,collector_kind) VALUES ('board','board')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO edge(id,source_id,target_id,kind) VALUES ('e9','top-photo','board','contains')",
            [],
        )
        .unwrap();

        let cols = workspace(&c, None, &[]).unwrap();
        assert_eq!(
            names(&cols[0]),
            vec!["Bergamo", "Loose", "My board"],
            "Cover is in the board now, and the board is at the top"
        );
    }

    #[test]
    fn a_cascade_can_start_at_a_folder_nested_inside_another() {
        // The reported blank column view: `Bergamo` is inside `Trips`, so it
        // is not in the library's root column, and a walk that began with it
        // used to stop before it started.
        let c = seed();
        let cols = tree_from(&c, Some("sub-folder"), &[]).unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].scope_id.as_deref(), Some("sub-folder"));
        assert_eq!(cols[0].title, "Bergamo");
        assert_eq!(
            cols[0].rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["nested-photo"]
        );
    }

    #[test]
    fn a_scoped_cascade_still_descends() {
        let c = seed();
        let cols = tree_from(&c, Some("root-folder"), &["sub-folder".into()]).unwrap();
        assert_eq!(
            cols.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            vec!["Trips", "Bergamo"]
        );
    }

    #[test]
    fn a_scope_that_no_longer_exists_gives_an_empty_first_column_not_an_error() {
        let c = seed();
        let cols = tree_from(&c, Some("gone"), &[]).unwrap();
        assert_eq!(cols.len(), 1);
        assert!(cols[0].rows.is_empty());
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
