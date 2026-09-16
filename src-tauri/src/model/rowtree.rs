//! The Library's listing: everything you have, once, in sections by kind.
//!
//! The Library used to offer a second shape — the folder tree, one branch
//! open at a time — and that is gone. A tree asks two questions at once
//! ("what do I have" and "where does it sit") and answers neither cleanly:
//! the same photograph appeared at the top level *and* under its folder,
//! expansion needed disclosure state the grid had nowhere to put, and the
//! arrow keys had to mean two different things depending on the shape.
//!
//! So: hierarchy lives in a collector, where it is the whole point and a
//! Miller cascade reads it like a filesystem. The Library is flat. A folder
//! is not scaffolding to be hidden here — it is one of the things you have,
//! and it gets a section of its own next to the images and the boards.
//!
//! What this module adds to `p_rows` is that sectioning. `type_group` in the
//! projection files every collector under one heading, which was right when
//! the tree carried the distinction and is not now: a board you arranged and
//! a folder mirrored from disk behave differently and belong apart. The split
//! is made here rather than in a view, so two views cannot disagree about it
//! (rule 1) — and here rather than in `projections.rs`, which is delivered
//! and takes additive changes only.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

use super::projections::{self, ListOptions, ListPage};

/// The two headings `p_rows` does not know about, named once so the ordering
/// below and the split above cannot drift apart.
pub const BOARDS: (&str, &str) = ("collector.board", "Collector boards");
pub const FOLDERS: (&str, &str) = ("collector.folder", "Collector folders");

/// The sections the Library is divided into, in the order they are drawn.
///
/// `p_rows` already names the first six; the two collector sections are this
/// module's refinement of its single "Collectors". `other` catches whatever
/// conforms to nothing above it, and is last because it is the one heading
/// that says nothing about what is under it.
const SECTIONS: &[(&str, &str)] = &[
    ("image", "Images"),
    ("video", "Video"),
    ("audio", "Audio"),
    ("model", "3D"),
    ("document", "Documents"),
    // Notes are documents in the ordinary sense and not in this model's: they
    // have their own node type and their own editor, so merging them into
    // Documents here would be this view disagreeing with the model about what
    // a note is.
    ("note", "Notes"),
    BOARDS,
    FOLDERS,
    ("other", "Other"),
];

fn section_rank(key: &str) -> usize {
    SECTIONS
        .iter()
        .position(|(k, _)| *k == key)
        .unwrap_or(SECTIONS.len())
}

/// Every item in the library, once, sectioned by what it is.
pub fn source(conn: &Connection, opts: &ListOptions) -> Result<ListPage> {
    let page = projections::rows(conn, &source_opts(opts))?;
    // A source switched off in the Sources panel is hidden, not forgotten.
    // The predicate lives in `sources` so every listing hides the same
    // things; this is the reader that filters in Rust rather than in SQL.
    let hidden = super::sources::hidden_ids(conn)?;
    let mut rows: Vec<_> = page.rows.into_iter().filter(|r| !hidden.contains(&r.id)).collect();

    // Only the type grouping is refined. Grouping by month or by health asks
    // a different question, and splitting the collectors inside those would
    // answer one it was not asked.
    if page.group_by == "type" {
        for row in rows.iter_mut() {
            if row.group_key != "collector" {
                continue;
            }
            // A collector with no recorded kind is a folder: that is what
            // `contains` makes it, and a board is the one that had to be
            // asked for.
            let (key, label) = match collector_kind(conn, &row.id)?.as_deref() {
                Some("board") => BOARDS,
                _ => FOLDERS,
            };
            row.group_key = key.to_string();
            row.group_label = label.to_string();
        }
        // Stable, so the sort `p_rows` applied inside each group survives:
        // this only moves whole sections, and separates the boards from the
        // folders without reordering either.
        rows.sort_by_key(|r| section_rank(&r.group_key));
    }

    for (i, row) in rows.iter_mut().enumerate() {
        // The Library draws no depth. A row inlined by an expanded collector
        // would arrive with one, and there is nothing here to read it.
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

/// `board` or `folder`, from the row the collector table already holds. None
/// when the node is not a collector, or is one nothing recorded a kind for.
fn collector_kind(conn: &Connection, id: &str) -> Result<Option<String>> {
    let kind = conn
        .query_row(
            "SELECT collector_kind FROM collector WHERE node_id = ?1",
            rusqlite::params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    Ok(kind)
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


#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        // Every migration, not just the model: `sources` lives in 002 and the
        // listings ask it what is switched off. A test database that is not
        // the real schema is a test that proves something else.
        crate::db::migrate(&c).unwrap();
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

    fn board(c: &Connection, id: &str, name: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name)
             VALUES (?1,'collector','app.archiva.collector.board','[\"app.archiva.collector.board\"]',?2)",
            params![id, name],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id, collector_kind) VALUES (?1,'board')",
            params![id],
        )
        .unwrap();
    }

    fn note(c: &Connection, id: &str, name: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,locator)
             VALUES (?1,'note','net.daringfireball.markdown','[\"net.daringfireball.markdown\"]',?2,?1)",
            params![id, name],
        )
        .unwrap();
    }

    /// Eight samples of silence, 8-bit mono at 8 kHz. Enough to be a file a
    /// browser will decode, which is the only thing a transport needs to
    /// prove.
    fn minimal_wav() -> Vec<u8> {
        let samples: [u8; 8] = [128; 8];
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36u32 + samples.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // chunk size
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&8000u32.to_le_bytes()); // sample rate
        out.extend_from_slice(&8000u32.to_le_bytes()); // byte rate
        out.extend_from_slice(&1u16.to_le_bytes()); // block align
        out.extend_from_slice(&8u16.to_le_bytes()); // bits per sample
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        out.extend_from_slice(&samples);
        out
    }

    /// One blank page, with a real cross-reference table so a viewer will
    /// open it rather than refuse.
    fn minimal_pdf() -> Vec<u8> {
        let objects = [
            "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n",
            "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n",
            "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n",
        ];
        let mut body = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for o in objects {
            offsets.push(body.len());
            body.push_str(o);
        }
        let xref_at = body.len();
        body.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
        for off in &offsets {
            body.push_str(&format!("{off:010} 00000 n \n"));
        }
        body.push_str(&format!(
            "trailer<</Size {}/Root 1 0 R>>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        ));
        body.into_bytes()
    }

    fn contains(c: &Connection, item: &str, folder: &str) {
        c.execute(
            "INSERT INTO edge(id,source_id,target_id,kind) VALUES (?1,?2,?3,'contains')",
            params![format!("e-{item}-{folder}"), item, folder],
        )
        .unwrap();
    }

    /// The Library's own options: sectioned by type, sorted by name — what
    /// the pane actually sends.
    fn opts(expanded: &[&str]) -> ListOptions {
        ListOptions {
            scope: None,
            group_by: "type".into(),
            sort: "name".into(),
            descending: false,
            expanded: expanded.iter().map(|s| s.to_string()).collect(),
            query: None,
        }
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
        // A real WAV and a real PDF, not placeholder bytes: the preview draws
        // a transport for one and the webview's own viewer for the other, and
        // a file that cannot be decoded proves neither. They also give the
        // Library an Audio and a Documents section to sort, which nothing
        // else in this tree does.
        let wav = minimal_wav();
        let pdf = minimal_pdf();
        for (rel, bytes) in [
            ("alpha.jpg", &b"a"[..]),
            ("zulu.jpg", &b"z"[..]),
            ("Trips/photo.jpg", &b"p"[..]),
            ("Trips/Bergamo/deep.jpg", &b"d"[..]),
            ("notes/thoughts.md", &b"# Thoughts\n\nSomething."[..]),
            ("clip.wav", &wav[..]),
            ("paper.pdf", &pdf[..]),
        ] {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(60)).unwrap();
        }

        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::db::migrate(&c).unwrap();
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
        // `paginate` is granted only when something recorded a page count —
        // the real extractor writes it, and the bare one used here does not.
        c.execute(
            "INSERT INTO attribute(node_id,key,value,value_num)
             SELECT id,'page_count','1',1 FROM node WHERE display_name = 'paper'",
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
        // Both sets of spines. A *scoped* workspace cascade is the inside of
        // one collector, which is the same listing either root gives — and
        // the Inspector's "open in Viewer" can hand it a watched root, which
        // is a start the workspace's own enumeration never reaches.
        for spine in w_spines.iter().chain(spines.iter()) {
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

        // Enough tagging history for the vocabulary rung to have something to
        // say. Three items carry harbour *and* boats; alpha carries only
        // harbour, so boats is a habit this one is missing. "Alpha" is a tag
        // nothing carries, named after the file on purpose — dull, but it is
        // the name-match rung, and the fixture's filenames are what they are.
        for (name, facet, on) in [
            ("harbour", "environment", &["alpha", "zulu", "photo", "deep"][..]),
            ("boats", "subject", &["zulu", "photo", "deep"][..]),
            ("Alpha", "subject", &[][..]),
        ] {
            let tag = crate::model::tags::ensure(&c, name, facet).unwrap();
            let ids: Vec<String> = on.iter().map(|n| id_of(n)).collect();
            if !ids.is_empty() {
                crate::model::tags::apply(&c, &ids, &tag).unwrap();
            }
        }
        crate::model::health::recompute_all(&c).unwrap();

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

        // And one for everything else in the library. The walkthrough follows
        // the active item wherever it lands — revealing something puts the
        // Inspector on it — so recording only the node this test happens to
        // click would report a disagreement that is really a gap in what was
        // written down.
        let mut records = serde_json::Map::new();
        {
            let mut q = c
                .prepare("SELECT id FROM node WHERE node_type <> 'tag'")
                .unwrap();
            let ids: Vec<String> = q
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            drop(q);
            for id in ids {
                records.insert(
                    id.clone(),
                    serde_json::to_value(crate::model::record::record(&c, &id).unwrap()).unwrap(),
                );
            }
        }

        // Every note's text, read before the tree below is deleted. The
        // text preview reads the file rather than the indexed copy, so a
        // harness driving it needs what the file actually said.
        let mut note_bodies = serde_json::Map::new();
        {
            let mut q = c
                .prepare("SELECT node_id FROM note")
                .unwrap();
            let ids: Vec<String> = q
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            drop(q);
            for id in ids {
                let body = crate::model::notetext::body(&c, &id).unwrap();
                note_bodies.insert(id, serde_json::to_value(body).unwrap());
            }
        }

        // A real space, described by the real code. The shell draws nothing
        // until one is open, so the walkthrough needs one — and a hand-written
        // shape here would be exactly the stub that once let a broken build
        // pass. Its folder is kept until the next run overwrites it, so
        // `reachable` is true the way it would be in the app.
        let space_home = std::env::temp_dir().join("archiva-fixture-space");
        std::fs::remove_dir_all(&space_home).ok();
        let space = crate::model::spaces::create(
            &space_home.join("app-data"),
            "Fixture Space",
            &space_home.join("Archive"),
        )
        .unwrap();
        let space_id = space.id.clone();
        let space_json = serde_json::to_value(crate::model::spaces::describe(
            space,
            Some(&space_id),
        ))
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
            "scoped": scoped,
            "workspace": workspace,
            "viewerRoot": viewer_root,
            "record": record,
            "records": records,
            "noteBodies": note_bodies,
            "space": space_json,
        });
        std::fs::write(&out, serde_json::to_string_pretty(&fixture).unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        eprintln!("fixture written to {out}");
    }

    fn sections(page: &ListPage) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for row in &page.rows {
            if out.last().map(String::as_str) != Some(row.group_label.as_str()) {
                out.push(row.group_label.clone());
            }
        }
        out
    }

    #[test]
    fn a_folder_is_something_you_have_rather_than_scaffolding_to_hide() {
        // It used to be dropped from the listing, because the tree beside it
        // was where folders lived. There is no tree now: a folder is one of
        // the things in the library, in a section of its own.
        let c = db();
        board(&c, "b", "My board");
        folder(&c, "f1", "Trips");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");

        let page = source(&c, &opts(&[])).unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.display_name.as_str()).collect::<Vec<_>>(),
            vec!["photo.jpg", "My board", "Trips"]
        );
        assert_eq!(page.total, 3);
    }

    #[test]
    fn boards_and_folders_are_separate_sections() {
        // p_rows files both under one "Collectors", which was right while the
        // tree carried the distinction. A board you arranged and a folder
        // mirrored from disk are different things to go looking for.
        let c = db();
        board(&c, "b1", "Moodboard");
        folder(&c, "f1", "Trips");
        item(&c, "p", "photo.jpg");

        let page = source(&c, &opts(&[])).unwrap();
        assert_eq!(sections(&page), vec!["Images", "Collector boards", "Collector folders"]);
        let keys: Vec<&str> = page.rows.iter().map(|r| r.group_key.as_str()).collect();
        assert_eq!(keys, vec!["image", BOARDS.0, FOLDERS.0]);
    }

    #[test]
    fn the_sections_come_in_a_fixed_order_whatever_the_library_holds() {
        let c = db();
        folder(&c, "f1", "Trips");
        board(&c, "b1", "Moodboard");
        item(&c, "p", "photo.jpg");
        note(&c, "n1", "thoughts");

        let page = source(&c, &opts(&[])).unwrap();
        assert_eq!(
            sections(&page),
            vec!["Images", "Notes", "Collector boards", "Collector folders"],
            "declared order, not the order things happened to be added"
        );
    }

    #[test]
    fn the_sort_inside_a_section_survives_the_resectioning() {
        let c = db();
        folder(&c, "f2", "Zermatt");
        folder(&c, "f1", "Alps");
        board(&c, "b1", "Moodboard");

        let page = source(&c, &opts(&[])).unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.display_name.as_str()).collect::<Vec<_>>(),
            vec!["Moodboard", "Alps", "Zermatt"],
            "boards first, and the folders still in name order"
        );
    }

    #[test]
    fn a_collector_with_no_recorded_kind_is_a_folder() {
        // `contains` is what makes a collector; a board is the one that had
        // to be asked for. An unknown kind must still land in a section.
        let c = db();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name)
             VALUES ('c','collector','app.archiva.collector','[]','Unlabelled')",
            [],
        )
        .unwrap();

        let page = source(&c, &opts(&[])).unwrap();
        assert_eq!(page.rows[0].group_key, FOLDERS.0);
    }

    #[test]
    fn grouping_by_something_else_is_left_alone() {
        // Splitting the collectors inside a health or month grouping would
        // answer a question that grouping was not asked.
        let c = db();
        board(&c, "b1", "Moodboard");
        folder(&c, "f1", "Trips");

        let mut o = opts(&[]);
        o.group_by = "health".into();
        let page = source(&c, &o).unwrap();
        assert!(
            page.rows.iter().all(|r| r.group_key != BOARDS.0 && r.group_key != FOLDERS.0),
            "{:?}",
            page.rows.iter().map(|r| r.group_key.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_scoped_listing_is_the_inside_of_one_collector() {
        // The Viewer's flat modes ask for this. Sectioning still applies —
        // it is the same listing, of fewer things.
        let c = db();
        folder(&c, "disk", "Photos");
        folder(&c, "sub", "Trips");
        item(&c, "p", "photo.jpg");
        item(&c, "o", "outside.jpg");
        contains(&c, "sub", "disk");
        contains(&c, "p", "disk");

        let mut o = opts(&[]);
        o.scope = Some("disk".into());
        let inside = source(&c, &o).unwrap();
        assert_eq!(
            inside.rows.iter().map(|r| r.display_name.as_str()).collect::<Vec<_>>(),
            vec!["photo.jpg", "Trips"],
            "what Photos holds, and nothing beside it"
        );
    }

    #[test]
    fn a_source_switched_off_takes_its_items_out_of_the_listing() {
        // Unticking a folder in the Sources panel hides what it holds. The
        // rows stay — ticking it again brings them back with their tags.
        let c = db();
        let photos = crate::model::sources::add(&c, "/photos").unwrap();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,locator)
             VALUES ('a','media','public.jpeg','[\"public.jpeg\",\"public.image\"]','a','/photos/a.jpg')",
            [],
        )
        .unwrap();
        item(&c, "loose", "loose.jpg");

        assert_eq!(source(&c, &opts(&[])).unwrap().rows.len(), 2);
        crate::model::sources::set_enabled(&c, &photos, false).unwrap();
        let page = source(&c, &opts(&[])).unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.display_name.as_str()).collect::<Vec<_>>(),
            vec!["loose.jpg"],
        );
        assert_eq!(page.total, 1, "and the count agrees with what is drawn");

        crate::model::sources::set_enabled(&c, &photos, true).unwrap();
        assert_eq!(source(&c, &opts(&[])).unwrap().rows.len(), 2, "back, untouched");
    }

    #[test]
    fn nothing_is_listed_twice() {
        // The duplication that made the tree unreadable: an item appeared at
        // the top level and again under its folder. Flat, it cannot.
        let c = db();
        folder(&c, "f1", "Trips");
        folder(&c, "f2", "Work");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");
        contains(&c, "p", "f2");

        let page = source(&c, &opts(&[])).unwrap();
        assert_eq!(
            page.rows.iter().filter(|r| r.id == "p").count(),
            1,
            "one row per node, however many collectors hold it"
        );
    }

    #[test]
    fn every_row_is_flat_and_ordinals_are_its_position() {
        let c = db();
        folder(&c, "f1", "Trips");
        item(&c, "p", "photo.jpg");
        contains(&c, "p", "f1");

        // `expanded` is what p_rows uses to inline a collector's members. The
        // Library never sends it, and a row that arrived nested would have
        // nowhere to be drawn.
        let page = source(&c, &opts(&["f1"])).unwrap();
        assert!(page.rows.iter().all(|r| r.depth == 0));
        assert_eq!(
            page.rows.iter().map(|r| r.ordinal).collect::<Vec<_>>(),
            (0..page.rows.len() as i64).collect::<Vec<_>>()
        );
    }
}
