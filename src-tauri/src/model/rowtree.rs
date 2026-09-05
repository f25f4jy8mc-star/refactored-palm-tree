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

/// Every item, once, with the folder structure left out.
///
/// The folders are structure rather than content — they say where things are,
/// not what you have — so a listing of what you have does not repeat them.
/// Collectors you made by hand stay: those are gatherings, and a gathering is
/// something you have.
pub fn source(conn: &Connection, opts: &ListOptions) -> Result<ListPage> {
    let page = projections::rows(conn, &source_opts(opts))?;
    // Only when listing the whole library. Inside a folder the subfolders are
    // its contents — dropping them there would list a folder's files and
    // silently lose everything in the folders beside them.
    let derived = if opts.scope.is_none() {
        super::folders::derived_ids(conn)?
    } else {
        std::collections::HashSet::new()
    };
    let mut rows: Vec<ListRow> = page
        .rows
        .into_iter()
        .filter(|r| !derived.contains(&r.id))
        .collect();
    for (i, row) in rows.iter_mut().enumerate() {
        row.depth = 0;
        row.ordinal = i as i64;
    }
    Ok(ListPage {
        total: rows.len(),
        rows,
        group_by: page.group_by,
        sort: page.sort,
    })
}

/// The two sections a hierarchy is divided into, and why: a watched folder
/// mirrors something on your disk and Archiva only reports it, while
/// everything else in the tree is something made here. Mixing them makes the
/// tree look like one filesystem you can edit anywhere in, and it isn't.
pub const WATCHED: (&str, &str) = ("watched", "Watched folders");
pub const MADE_HERE: (&str, &str) = ("archiva", "In Archiva");

pub fn hierarchy(conn: &Connection, opts: &ListOptions) -> Result<ListPage> {
    // The projection does the listing. `expanded` is cleared because the
    // inlining it does is exactly what this module is replacing, and grouping
    // is cleared because a tree's sections are watched-versus-made-here, not
    // whatever a flat list would group by.
    let base = projections::rows(conn, &ungrouped(opts, opts.scope.clone()))?;

    // An unscoped tree starts at what nothing contains. A scoped one is
    // already the inside of a collector, so its members are its roots.
    let derived = super::folders::derived_ids(conn)?;
    let roots: Vec<ListRow> = if opts.scope.is_some() {
        base.rows
    } else {
        let held = contained(conn)?;
        let mut roots: Vec<ListRow> = base
            .rows
            .into_iter()
            .filter(|r| !held.contains(&r.id))
            .collect();
        for row in roots.iter_mut() {
            let (key, label) = if derived.contains(&row.id) {
                WATCHED
            } else {
                MADE_HERE
            };
            row.group_key = key.to_string();
            row.group_label = label.to_string();
        }
        // Watched folders first: they are where the material comes from.
        roots.sort_by_key(|r| u8::from(r.group_key != WATCHED.0));
        roots
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
    let members = projections::rows(conn, &ungrouped(opts, Some(id)))?;
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

/// The same options, listing one scope with no inlining and no grouping —
/// the tree supplies its own sections, and a folder's contents are already
/// grouped by being in that folder.
fn ungrouped(opts: &ListOptions, scope: Option<String>) -> ListOptions {
    ListOptions {
        scope,
        group_by: "none".into(),
        sort: opts.sort.clone(),
        descending: opts.descending,
        expanded: Vec::new(),
        query: opts.query.clone(),
    }
}

/// Source keeps whatever grouping the pane asked for — it is a flat listing,
/// and grouping it by type or month is exactly what that control is for.
fn source_opts(opts: &ListOptions) -> ListOptions {
    ListOptions {
        scope: opts.scope.clone(),
        group_by: opts.group_by.clone(),
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

    /// Writes the exact rows the real backend produces for a real scanned
    /// directory tree, so the interface walkthrough can be driven by what the
    /// Rust actually returns instead of a shape someone hand-wrote.
    ///
    /// This exists because a hand-written stub let a broken build pass: the
    /// harness answered with the tree it was told to, the backend answered
    /// with something else, and nothing compared the two. Run with
    /// ARCHIVA_FIXTURE=<path>.
    #[test]
    fn emit_fixture_for_the_interface_walkthrough() {
        use crate::model::{folders, scan};
        use std::time::{Duration, SystemTime};

        let Ok(out) = std::env::var("ARCHIVA_FIXTURE") else {
            return;
        };

        struct Bare;
        impl scan::Extractor for Bare {
            fn extract(&self, _p: &std::path::Path, _ct: &str) -> Vec<(String, Option<String>, Option<f64>)> {
                vec![]
            }
            fn version(&self) -> i64 {
                1
            }
            fn proxies(&self, _p: &std::path::Path, _ct: &str, _h: Option<&str>) -> scan::Proxies {
                scan::Proxies::not_applicable(1)
            }
        }

        let dir = std::env::temp_dir().join("archiva-fixture-tree");
        std::fs::remove_dir_all(&dir).ok();
        for (rel, bytes) in [
            ("alpha.jpg", &b"a"[..]),
            ("zulu.jpg", &b"z"[..]),
            ("Trips/photo.jpg", &b"p"[..]),
            ("Trips/Bergamo/deep.jpg", &b"d"[..]),
            ("notes/thoughts.md", &b"# Thoughts\n\nSomething."[..]),
        ] {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(60)).unwrap();
        }

        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql")).unwrap();
        scan::scan(&mut c, &[dir.clone()], &[], &Bare).unwrap();
        folders::rebuild(&c, &[dir.clone()]).unwrap();
        // One Collector made here rather than mirrored from disk, so the
        // tree's two sections both have something in them.
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,
                              display_subtitle,icon_kind,source_kind)
             VALUES ('made-here','collector','app.archiva.virtual','[]','My board',
                     'board','folder','app_generated')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id,collector_kind) VALUES ('made-here','board')",
            [],
        )
        .unwrap();
        crate::model::health::recompute_all(&c).unwrap();

        let id_of = |name: &str| -> String {
            c.query_row(
                "SELECT id FROM node WHERE display_name = ?1",
                params![name],
                |r| r.get(0),
            )
            .unwrap_or_else(|e| panic!("no node called {name}: {e}"))
        };
        let root_name = dir.file_name().unwrap().to_string_lossy().to_string();
        let ids = serde_json::json!({
            "root": id_of(&root_name),
            "trips": id_of("Trips"),
            "bergamo": id_of("Bergamo"),
            "alpha": id_of("alpha"),
            "zulu": id_of("zulu"),
            "photo": id_of("photo"),
            "deep": id_of("deep"),
        });

        let base = |expanded: Vec<String>| ListOptions {
            scope: None,
            group_by: "none".into(),
            sort: "name".into(),
            descending: false,
            expanded,
            query: None,
        };
        // Every open-set the walkthrough can reach. Only one branch is open
        // at a time, so a spine is a path down the folder tree — enumerate
        // them from the folders themselves rather than listing the ones this
        // test happens to click, or the harness reports a disagreement that
        // is really just an unrecorded answer.
        let mut spines: Vec<Vec<String>> = vec![vec![]];
        {
            let mut frontier: Vec<Vec<String>> = vec![vec![]];
            while let Some(spine) = frontier.pop() {
                let scope = spine.last().cloned();
                let mut q = c
                    .prepare(match scope {
                        Some(_) => {
                            "SELECT n.id FROM node n
                               JOIN edge e ON e.source_id = n.id AND e.kind='contains'
                                          AND e.target_id = ?1
                              WHERE n.node_type = 'collector'"
                        }
                        None => {
                            "SELECT n.id FROM node n
                              WHERE n.node_type = 'collector' AND ?1 IS NULL
                                AND NOT EXISTS (SELECT 1 FROM edge e
                                                 WHERE e.kind='contains' AND e.source_id = n.id)"
                        }
                    })
                    .unwrap();
                let kids: Vec<String> = q
                    .query_map(params![scope], |r| r.get(0))
                    .unwrap()
                    .collect::<std::result::Result<_, _>>()
                    .unwrap();
                drop(q);
                for kid in kids {
                    let mut next = spine.clone();
                    next.push(kid);
                    spines.push(next.clone());
                    frontier.push(next);
                }
            }
        }
        // The Viewer's cascade starts inside the watched folders, so it can
        // reach spines the Library's cannot — a folder one level down is a
        // *root* there. Enumerating from the Library's roots alone left those
        // unrecorded, and the walkthrough reported it as a disagreement,
        // which is precisely what it is for.
        let mut w_spines: Vec<Vec<String>> = vec![vec![]];
        {
            let mut frontier: Vec<Vec<String>> = crate::model::tree::workspace(&c, None, &[])
                .unwrap()[0]
                .rows
                .iter()
                .filter(|r| r.node_type == "collector")
                .map(|r| vec![r.id.clone()])
                .collect();
            w_spines.extend(frontier.iter().cloned());
            while let Some(spine) = frontier.pop() {
                let scope = spine.last().cloned();
                let mut q = c
                    .prepare(
                        "SELECT n.id FROM node n
                           JOIN edge e ON e.source_id = n.id AND e.kind='contains'
                                      AND e.target_id = ?1
                          WHERE n.node_type = 'collector'",
                    )
                    .unwrap();
                let kids: Vec<String> = q
                    .query_map(params![scope], |r| r.get(0))
                    .unwrap()
                    .collect::<std::result::Result<_, _>>()
                    .unwrap();
                drop(q);
                for kid in kids {
                    let mut next = spine.clone();
                    next.push(kid);
                    w_spines.push(next.clone());
                    frontier.push(next);
                }
            }
        }

        let mut hierarchy = serde_json::Map::new();
        for spine in &spines {
            let key = spine.join(",");
            hierarchy.insert(
                key,
                serde_json::to_value(hierarchy_page(&c, &base(spine.clone()))).unwrap(),
            );
        }

        // The cascade from every start the walkthrough can reach: from the
        // library root, and from each folder as its own root — the case that
        // came up blank and the reason `tree_from` takes one. `workspace` is
        // the same walks as the Viewer asks for them, where the watched
        // folders are not drawn and their contents stand in their place.
        let mut scoped = serde_json::Map::new();
        for spine in &spines {
            scoped.insert(
                format!("|{}", spine.join("|")),
                serde_json::to_value(crate::model::tree::tree_from(&c, None, spine).unwrap())
                    .unwrap(),
            );
            if let Some((root, rest)) = spine.split_first() {
                scoped.insert(
                    format!("{}|{}", root, rest.join("|")),
                    serde_json::to_value(
                        crate::model::tree::tree_from(&c, Some(root), rest).unwrap(),
                    )
                    .unwrap(),
                );
            }
        }
        let mut workspace = serde_json::Map::new();
        for spine in &w_spines {
            workspace.insert(
                format!("|{}", spine.join("|")),
                serde_json::to_value(crate::model::tree::workspace(&c, None, spine).unwrap())
                    .unwrap(),
            );
            if let Some((root, rest)) = spine.split_first() {
                workspace.insert(
                    format!("{}|{}", root, rest.join("|")),
                    serde_json::to_value(
                        crate::model::tree::workspace(&c, Some(root), rest).unwrap(),
                    )
                    .unwrap(),
                );
            }
        }

        // Taken before the compass edges below, so the listings recorded here
        // are the listings of a plain scan and nothing else.
        let source_page = serde_json::to_value(source(&c, &base(vec![])).unwrap()).unwrap();
        let source_by_type = serde_json::to_value(
            source(&c, &ListOptions { group_by: "type".into(), ..base(vec![]) }).unwrap(),
        )
        .unwrap();

        // A compass, from the real thing. The Inspector's cross is drawn
        // entirely out of `p_record`'s slots, so a hand-written `slots: []`
        // would prove nothing about it — the lesson recorded in CLAUDE.md.
        // North and West are given links and South and East are left empty on
        // purpose: an arm with nothing in it is the case the cross has to keep
        // drawing.
        for (source_id, target_id, kind, raw) in [
            (id_of("zulu"), id_of("alpha"), "compass_s", None), // alpha's North holds zulu
            (id_of("alpha"), id_of("photo"), "compass_w", None),
            // A wikilink is West too (§1.7), and carries the text it was
            // written as — the schema insists on it.
            (id_of("alpha"), id_of("thoughts"), "wikilink", Some("Thoughts")),
        ] {
            c.execute(
                "INSERT INTO edge(id,source_id,target_id,kind,origin,raw_target)
                 VALUES (?1,?2,?3,?4,'user',?5)",
                params![crate::model::scan::uuid_v7(), source_id, target_id, kind, raw],
            )
            .unwrap();
        }
        let record =
            serde_json::to_value(crate::model::record::record(&c, &id_of("alpha")).unwrap())
                .unwrap();

        // The Viewer's own root, spelled out: what its cascade opens with
        // when nothing scopes it. The walkthrough checks this against the
        // watched folder it must not be showing.
        let viewer_root =
            serde_json::to_value(crate::model::tree::workspace(&c, None, &[]).unwrap()).unwrap();

        let fixture = serde_json::json!({
            "rootName": root_name,
            "ids": ids,
            "source": source_page,
            "sourceByType": source_by_type,
            "hierarchy": hierarchy,
            "scoped": scoped,
            "workspace": workspace,
            "viewerRoot": viewer_root,
            "record": record,
        });
        std::fs::write(&out, serde_json::to_string_pretty(&fixture).unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        eprintln!("fixture written to {out}");
    }

    fn hierarchy_page(c: &Connection, o: &ListOptions) -> ListPage {
        hierarchy(c, o).unwrap()
    }

    #[test]
    fn the_root_holds_only_what_nothing_contains() {
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "a", "alpha.jpg");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");

        let page = hierarchy(&c, &opts(&[])).unwrap();
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

        let open = hierarchy(&c, &opts(&["f1"])).unwrap();
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

        let shut = hierarchy(&c, &opts(&[])).unwrap();
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

        let page = hierarchy(&c, &opts(&["f1", "f2"])).unwrap();
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

        let page = hierarchy(&c, &opts(&["f2"])).unwrap();
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

        let page = hierarchy(&c, &opts(&["f1", "f2"])).unwrap();
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

        let page = hierarchy(&c, &opts(&["f1", "f2"])).unwrap();
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
        let page = hierarchy(&c, &o).unwrap();
        assert_eq!(
            shape(&page),
            vec![("Bergamo".into(), 0), ("photo.jpg".into(), 1)],
            "the pane is the inside of Trips; outside.jpg is not in it"
        );
    }

    #[test]
    fn the_tree_separates_watched_folders_from_what_was_made_here() {
        let c = db();
        // A folder the folder pass made, standing for something on disk.
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,
                              source_kind,locator)
             VALUES ('disk','collector','app.archiva.virtual','[]','Photos',
                     'app_generated','/photos')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id,collector_kind) VALUES ('disk','folder')",
            [],
        )
        .unwrap();
        // One made here: no locator, so the folder pass does not own it.
        folder(&c, "mine", "My board");

        let page = hierarchy(&c, &opts(&[])).unwrap();
        let sections: Vec<(&str, &str)> = page
            .rows
            .iter()
            .map(|r| (r.display_name.as_str(), r.group_key.as_str()))
            .collect();
        assert_eq!(
            sections,
            vec![("Photos", WATCHED.0), ("My board", MADE_HERE.0)],
            "watched first, and never mixed"
        );
        assert_eq!(page.rows[0].group_label, WATCHED.1);
    }

    #[test]
    fn a_folders_contents_stay_in_their_folders_section() {
        let c = db();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,
                              source_kind,locator)
             VALUES ('disk','collector','app.archiva.virtual','[]','Photos',
                     'app_generated','/photos')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id,collector_kind) VALUES ('disk','folder')",
            [],
        )
        .unwrap();
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "disk");

        let page = hierarchy(&c, &opts(&["disk"])).unwrap();
        assert!(page.rows.iter().all(|r| r.group_key == WATCHED.0));
    }

    #[test]
    fn a_group_control_does_not_scatter_a_folders_contents() {
        // Group-by belongs to Source. A tree asked to group by type would
        // otherwise file a folder's contents under headers the folder is not
        // itself in, which is unreadable.
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");

        let mut o = opts(&["f1"]);
        o.group_by = "type".into();
        let page = hierarchy(&c, &o).unwrap();
        let groups: Vec<&String> = page.rows.iter().map(|r| &r.group_key).collect();
        assert_eq!(groups[0], groups[1]);
    }

    #[test]
    fn ordinals_stay_the_rows_position_in_the_page() {
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "a", "alpha.jpg");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");
        let page = hierarchy(&c, &opts(&["f1"])).unwrap();
        let ordinals: Vec<i64> = page.rows.iter().map(|r| r.ordinal).collect();
        assert_eq!(ordinals, vec![0, 1, 2]);
        assert_eq!(page.total, 3);
    }

    #[test]
    fn a_scoped_source_listing_keeps_the_subfolders_inside_it() {
        // The whole library leaves the folder scaffolding out; the inside of
        // a folder cannot, or half its contents disappear.
        let c = db();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,
                              source_kind,locator)
             VALUES ('disk','collector','app.archiva.collector.folder','[]','Photos',
                     'app_generated','/photos')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id,collector_kind) VALUES ('disk','folder')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,
                              source_kind,locator)
             VALUES ('sub','collector','app.archiva.collector.folder','[]','Trips',
                     'app_generated','/photos/trips')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id,collector_kind) VALUES ('sub','folder')",
            [],
        )
        .unwrap();
        contains(&c, "sub", "disk");

        let mut o = opts(&[]);
        o.scope = Some("disk".into());
        let inside = source(&c, &o).unwrap();
        assert_eq!(
            inside.rows.iter().map(|r| r.display_name.as_str()).collect::<Vec<_>>(),
            vec!["Trips"],
            "a subfolder is part of what a folder holds"
        );

        let whole = source(&c, &opts(&[])).unwrap();
        assert!(whole.rows.is_empty(), "and the library itself leaves them out");
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
