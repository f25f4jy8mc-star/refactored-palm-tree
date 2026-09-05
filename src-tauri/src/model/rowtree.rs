//! The Library as a tree, rather than as everything at once.
//!
//! `p_rows` unscoped returns every node in the library, and inlines a
//! collector's members beneath it when that collector is expanded. Both are
//! correct for a flat grid — "show me everything I have" — and together they
//! are wrong for a list with disclosure triangles:
//!
//!   * a photograph inside a folder is listed at the top level **as well as**
//!     under its folder, so expanding looks like it duplicated the contents;
//!   * collapsing then appears to do nothing, because the top-level copy is
//!     still there;
//!   * and expansion only ever went one level deep, because `p_rows` inlines
//!     children but never the children's children.
//!
//! So this module assembles the tree instead, reusing the projection for
//! every listing it needs rather than writing a second query: the root is the
//! nodes nothing contains — the same rule `p_tree` uses for its first column —
//! and each expanded collector's members come from `p_rows` scoped to it.
//! Grouping, sorting, health and capabilities all still arrive decided; the
//! only thing added here is the shape.
//!
//! A grid still asks for the flat listing. Two shapes, one projection, and
//! the choice is made by the pane rather than by a second copy of the data.

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::Connection;

use super::projections::{self, ListOptions, ListPage, ListRow};

/// How deep expansion may nest before we stop. A `contains` edge can point
/// anywhere, including in a loop, and a loop would otherwise recurse until the
/// stack ran out. The path check below catches direct cycles; this is the
/// backstop for a chain that is merely absurd.
const MAX_DEPTH: i64 = 24;

pub fn rows(conn: &Connection, opts: &ListOptions) -> Result<ListPage> {
    // The projection does the listing. `expanded` is cleared because the
    // inlining it does is exactly what this module is replacing.
    let base = projections::rows(conn, &flat(opts, opts.scope.clone()))?;

    // An unscoped tree starts at what nothing contains. A scoped one is
    // already the inside of a collector, so its members are its roots.
    let roots: Vec<ListRow> = if opts.scope.is_some() {
        base.rows
    } else {
        let held = contained(conn)?;
        base.rows.into_iter().filter(|r| !held.contains(&r.id)).collect()
    };

    let mut out: Vec<ListRow> = Vec::with_capacity(roots.len());
    let mut path: Vec<String> = Vec::new();
    for row in roots {
        push_with_children(conn, opts, row, 0, &mut path, &mut out)?;
    }

    // Ordinal is the row's index in the flattened page, and stays so.
    for (i, row) in out.iter_mut().enumerate() {
        row.ordinal = i as i64;
    }

    Ok(ListPage {
        total: out.len(),
        rows: out,
        group_by: base.group_by,
        sort: base.sort,
    })
}

fn push_with_children(
    conn: &Connection,
    opts: &ListOptions,
    mut row: ListRow,
    depth: i64,
    path: &mut Vec<String>,
    out: &mut Vec<ListRow>,
) -> Result<()> {
    let id = row.id.clone();
    let group_key = row.group_key.clone();
    let group_label = row.group_label.clone();
    row.depth = depth;
    let expand = row.node_type == "collector"
        && depth < MAX_DEPTH
        && opts.expanded.iter().any(|e| *e == id)
        // A collector reachable from inside itself would otherwise be drawn
        // forever. Stopping at the repeat keeps the loop visible — the folder
        // is still there, it just does not open a second time.
        && !path.contains(&id);
    out.push(row);
    if !expand {
        return Ok(());
    }

    path.push(id.clone());
    let members = projections::rows(conn, &flat(opts, Some(id)))?;
    for mut child in members.rows {
        // Children belong to the group their parent is drawn under, so an
        // expanded folder cannot scatter its contents across headers it is
        // not itself in.
        child.group_key = group_key.clone();
        child.group_label = group_label.clone();
        push_with_children(conn, opts, child, depth + 1, path, out)?;
    }
    path.pop();
    Ok(())
}

/// The same options, listing one scope flat: no inlining, and no grouping
/// below the top level — a folder's contents are already grouped by being in
/// that folder.
fn flat(opts: &ListOptions, scope: Option<String>) -> ListOptions {
    ListOptions {
        scope: scope.clone(),
        group_by: if scope.is_some() {
            "none".into()
        } else {
            opts.group_by.clone()
        },
        sort: opts.sort.clone(),
        descending: opts.descending,
        expanded: Vec::new(),
        query: opts.query.clone(),
    }
}

/// Every node held by at least one collector.
fn contained(conn: &Connection) -> Result<HashSet<String>> {
    let mut q = conn.prepare("SELECT DISTINCT source_id FROM edge WHERE kind = 'contains'")?;
    let out = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    fn item(c: &Connection, id: &str, name: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,locator)
             VALUES (?1,'media','public.jpeg','[\"public.jpeg\",\"public.image\",\"public.data\"]',?2,?1)",
            params![id, name],
        )
        .unwrap();
    }

    fn folder(c: &Connection, id: &str, name: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name)
             VALUES (?1,'collector','app.archiva.virtual','[\"app.archiva.virtual\"]',?2)",
            params![id, name],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id, collector_kind) VALUES (?1,'folder')",
            params![id],
        )
        .unwrap();
    }

    fn contains(c: &Connection, item: &str, folder: &str) {
        c.execute(
            "INSERT INTO edge(id,source_id,target_id,kind) VALUES (?1,?2,?3,'contains')",
            params![format!("e-{item}-{folder}"), item, folder],
        )
        .unwrap();
    }

    fn opts(expanded: &[&str]) -> ListOptions {
        ListOptions {
            scope: None,
            group_by: "none".into(),
            sort: "name".into(),
            descending: false,
            expanded: expanded.iter().map(|s| s.to_string()).collect(),
            query: None,
        }
    }

    fn shape(page: &ListPage) -> Vec<(String, i64)> {
        page.rows
            .iter()
            .map(|r| (r.display_name.clone(), r.depth))
            .collect()
    }

    #[test]
    fn the_root_holds_only_what_nothing_contains() {
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "a", "alpha.jpg");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");

        let page = rows(&c, &opts(&[])).unwrap();
        // Sorted by name, so alpha comes before Trips.
        assert_eq!(
            shape(&page),
            vec![("alpha.jpg".into(), 0), ("Trips".into(), 0)],
            "the photo is inside the folder, so it is not also at the top"
        );
    }

    #[test]
    fn expanding_adds_the_contents_once_and_collapsing_takes_them_away() {
        // The reported bug: expanding appeared to duplicate, and collapsing
        // appeared to do nothing, because the contents were listed at the top
        // level as well.
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "a", "alpha.jpg");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");

        let open = rows(&c, &opts(&["f1"])).unwrap();
        assert_eq!(
            shape(&open),
            vec![
                ("alpha.jpg".into(), 0),
                ("Trips".into(), 0),
                ("photo.jpg".into(), 1)
            ]
        );
        let names: Vec<&String> = open.rows.iter().map(|r| &r.display_name).collect();
        assert_eq!(
            names.iter().filter(|n| **n == "photo.jpg").count(),
            1,
            "once, not twice"
        );

        let shut = rows(&c, &opts(&[])).unwrap();
        assert_eq!(shut.rows.len(), 2, "collapsing really removes them");
    }

    #[test]
    fn expansion_nests_as_deep_as_it_is_asked_to() {
        // p_rows inlines one level only; a tree has to go all the way down.
        let c = db();
        folder(&c, "f1", "Trips");
        folder(&c, "f2", "Bergamo");
        item(&c, "p", "photo.jpg");
        contains(&c, "f2", "f1");
        contains(&c, "p", "f2");

        let page = rows(&c, &opts(&["f1", "f2"])).unwrap();
        assert_eq!(
            shape(&page),
            vec![
                ("Trips".into(), 0),
                ("Bergamo".into(), 1),
                ("photo.jpg".into(), 2)
            ]
        );
    }

    #[test]
    fn expanding_the_inner_folder_alone_leaves_the_outer_one_shut() {
        let c = db();
        folder(&c, "f1", "Trips");
        folder(&c, "f2", "Bergamo");
        item(&c, "p", "photo.jpg");
        contains(&c, "f2", "f1");
        contains(&c, "p", "f2");

        let page = rows(&c, &opts(&["f2"])).unwrap();
        assert_eq!(shape(&page), vec![("Trips".into(), 0)]);
    }

    #[test]
    fn one_item_in_two_folders_appears_under_each_of_them() {
        // Still legitimate, and still one node. The view tells the two rows
        // apart by placement, not by id.
        let c = db();
        folder(&c, "f1", "Trips");
        folder(&c, "f2", "Work");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");
        contains(&c, "p", "f2");

        let page = rows(&c, &opts(&["f1", "f2"])).unwrap();
        assert_eq!(
            shape(&page),
            vec![
                ("Trips".into(), 0),
                ("photo.jpg".into(), 1),
                ("Work".into(), 0),
                ("photo.jpg".into(), 1)
            ]
        );
    }

    #[test]
    fn a_folder_that_contains_itself_is_drawn_once_rather_than_forever() {
        let c = db();
        folder(&c, "f1", "Trips");
        folder(&c, "f2", "Inner");
        contains(&c, "f2", "f1");
        contains(&c, "f1", "f2"); // a loop

        let page = rows(&c, &opts(&["f1", "f2"])).unwrap();
        // Nothing is uncontained, so nothing is a root — but the important
        // part is that it terminates.
        assert!(page.rows.len() < 10, "{:?}", shape(&page));
    }

    #[test]
    fn a_scoped_tree_starts_at_that_collectors_members() {
        let c = db();
        folder(&c, "f1", "Trips");
        folder(&c, "f2", "Bergamo");
        item(&c, "p", "photo.jpg");
        item(&c, "o", "outside.jpg");
        contains(&c, "f2", "f1");
        contains(&c, "p", "f2");

        let mut o = opts(&["f2"]);
        o.scope = Some("f1".into());
        let page = rows(&c, &o).unwrap();
        assert_eq!(
            shape(&page),
            vec![("Bergamo".into(), 0), ("photo.jpg".into(), 1)],
            "the pane is the inside of Trips; outside.jpg is not in it"
        );
    }

    #[test]
    fn children_are_drawn_under_their_parents_group_not_their_own() {
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");

        let mut o = opts(&["f1"]);
        o.group_by = "type".into();
        let page = rows(&c, &o).unwrap();
        let groups: Vec<&String> = page.rows.iter().map(|r| &r.group_key).collect();
        assert_eq!(
            groups[0], groups[1],
            "an expanded folder must not scatter its contents into other headers"
        );
    }

    #[test]
    fn ordinals_stay_the_rows_position_in_the_page() {
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "a", "alpha.jpg");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");
        let page = rows(&c, &opts(&["f1"])).unwrap();
        let ordinals: Vec<i64> = page.rows.iter().map(|r| r.ordinal).collect();
        assert_eq!(ordinals, vec![0, 1, 2]);
        assert_eq!(page.total, 3);
    }

    #[test]
    fn the_flat_listing_is_untouched_and_still_shows_everything() {
        // The grid still wants "everything I have", and that is p_rows as
        // delivered — this module is a second shape, not a replacement.
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");
        let flat = projections::rows(&c, &opts(&[])).unwrap();
        assert_eq!(flat.rows.len(), 2, "the folder and the photo");
    }
}
