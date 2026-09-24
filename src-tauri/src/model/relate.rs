//! Relating and making things, from the interface.
//!
//! `mutations` is the named write path (S11) and says what one edge means.
//! This module is what the interface asks for in whole gestures — "put these
//! three in this item's West", "gather the tray into this folder", "make a note
//! here" — each done in one transaction and reported in one answer, so the
//! command that calls it can emit one change event (invariant 10).
//!
//! It decides nothing `mutations` already decides. Every edge goes through
//! `mutations::link`, so the drop zone still chooses the kind (S6): a tag put
//! in North is a tagging, a collector put in North is membership, anything
//! else is a compass link. Writing kinds here would be a second opinion.
//!
//! What it does decide, once, so no two views can disagree about it (rule 1):
//!
//!   * **Which collectors can be gathered into.** A folder mirrored from disk
//!     is a description of where files sit; putting something "in" it would be
//!     a claim the disk contradicts and the next scan cannot see. Only a
//!     collector made here takes members. `gather_target` is the one answer
//!     every surface asks for.
//!   * **Where a new note lives.** In the space's own `notes/` folder, as a
//!     real `.md` file (S9) — and recorded `app_generated`, because the scan
//!     never walks the space (invariant 9) and would otherwise report a note it
//!     cannot see as missing on the next Refresh.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::content_type;
use super::health;
use super::mutations;
use super::projections::{self, Row};
use super::scan::uuid_v7;

/* ------------------------------------------------------------- linking */

/// What a batch of links came to. Counted rather than returned as a bare
/// success, because "3 added, 1 already there" and "4 added" are different
/// things to be told — and a refusal is reported beside the rest rather than
/// failing the whole gesture over one item that cannot be linked to itself.
#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    pub created: usize,
    pub existed: usize,
    /// Each refused id with the reason, in the order they were given.
    pub refused: Vec<(String, String)>,
}

/// Put each of `others` in `item`'s `compass` arm.
///
/// `link(item, compass, other)`: the edge is written from the item being
/// looked at, which is what "put this in its North" means, and the far end
/// reads it inverted or not by the rule in `projections::reciprocal` (S5).
pub fn add_to_arm(conn: &Connection, item: &str, compass: &str, others: &[String]) -> Result<Added> {
    let tx = conn.unchecked_transaction()?;
    let mut out = Added::default();
    for other in others {
        match mutations::link(&tx, item, compass, other, None, None) {
            Ok(l) if l.existed => out.existed += 1,
            Ok(_) => out.created += 1,
            // The refusals `link` makes are about this one pair — itself, a
            // direction that is not one — so they are reported, not fatal.
            Err(e) => out.refused.push((other.clone(), e.to_string())),
        }
    }
    let mut touched: Vec<String> = others.to_vec();
    touched.push(item.to_string());
    // A tag in North changes what the item has filled in; a link changes
    // nothing health counts today but the recompute is what keeps "today"
    // from becoming an assumption.
    health::recompute_many(&tx, &existing_ids(&tx, &touched)?)?;
    tx.commit()?;
    Ok(out)
}

/// Remove one edge, whichever end it was written from, and recompute both.
pub fn unlink(conn: &Connection, edge_id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let ends: (String, Option<String>) = tx
        .query_row(
            "SELECT source_id, target_id FROM edge WHERE id = ?1",
            params![edge_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| anyhow!("no such link: {edge_id}"))?;
    mutations::unlink(&tx, edge_id)?;
    let mut touched = vec![ends.0];
    touched.extend(ends.1);
    health::recompute_many(&tx, &touched)?;
    tx.commit()?;
    Ok(())
}

/* ----------------------------------------------------------- gathering */

/// A collector that can take members. The name travels with it so the
/// interface can say "Add 3 to Moodboard" without a second call.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GatherTarget {
    pub id: String,
    pub name: String,
    /// `folder` or `board`.
    pub kind: String,
}

/// Whether `id` is a collector made here, and so something you can gather
/// into. `None` for anything else — an item, a tag, a folder mirrored from a
/// linked one. See the module note for why the mirrored folder is refused.
pub fn gather_target(conn: &Connection, id: &str) -> Result<Option<GatherTarget>> {
    let row: Option<(String, Option<String>, String, Option<String>)> = conn
        .query_row(
            "SELECT n.display_name, n.locator, n.source_kind, c.collector_kind
               FROM node n JOIN collector c ON c.node_id = n.id
              WHERE n.id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((name, locator, source_kind, kind)) = row else {
        return Ok(None);
    };
    // The same test `folders::derived_ids` makes: app_generated *with* a path
    // is a folder mirrored from disk.
    if source_kind == "app_generated" && locator.is_some() {
        return Ok(None);
    }
    Ok(Some(GatherTarget {
        id: id.to_string(),
        name,
        kind: kind.unwrap_or_else(|| "folder".into()),
    }))
}

/// Every collector you can gather into, by name. The tag popup's collector
/// column, and nothing else needs to know what makes one eligible.
pub fn gather_targets(conn: &Connection) -> Result<Vec<GatherTarget>> {
    let mut q = conn.prepare(
        "SELECT n.id FROM node n JOIN collector c ON c.node_id = n.id
          ORDER BY n.display_name COLLATE NOCASE",
    )?;
    let ids: Vec<String> = q
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    drop(q);
    let mut out = Vec::new();
    for id in ids {
        if let Some(t) = gather_target(conn, &id)? {
            out.push(t);
        }
    }
    Ok(out)
}

/// Put `ids` in `collector`. Through the write path, as North of each item —
/// which is what membership *is* (S6), so this file never writes `contains`.
pub fn gather(conn: &Connection, ids: &[String], collector: &str) -> Result<Added> {
    let target = gather_target(conn, collector)?.ok_or_else(|| {
        anyhow!("only a collector made in Archiva can take members — a linked folder mirrors the disk")
    })?;
    let tx = conn.unchecked_transaction()?;
    let mut out = Added::default();
    for id in ids {
        match mutations::link(&tx, id, "N", &target.id, None, None) {
            Ok(l) if l.existed => out.existed += 1,
            Ok(_) => out.created += 1,
            Err(e) => out.refused.push((id.clone(), e.to_string())),
        }
    }
    tx.commit()?;
    Ok(out)
}

/// Take `ids` out of `collector`. Returns how many were in it.
pub fn ungather(conn: &Connection, ids: &[String], collector: &str) -> Result<usize> {
    if gather_target(conn, collector)?.is_none() {
        return Err(anyhow!("that collector's contents are not yours to change here"));
    }
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for id in ids {
        let edge: Option<String> = tx
            .query_row(
                "SELECT id FROM edge WHERE source_id = ?1 AND target_id = ?2 AND kind = 'contains'",
                params![id, collector],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(e) = edge {
            mutations::unlink(&tx, &e)?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

/* ------------------------------------------------------------- reading */

/// The rows for a set of ids, in the order asked, skipping any that no longer
/// exist. The tray and the tag popup hold ids a list published; this is how
/// they draw them without each re-deriving a row from raw columns.
pub fn rows_of(conn: &Connection, ids: &[String]) -> Result<Vec<Row>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let exists: bool = conn
            .query_row("SELECT 1 FROM node WHERE id = ?1", params![id], |_| Ok(()))
            .optional()?
            .is_some();
        if exists {
            out.push(projections::row(conn, id)?);
        }
    }
    Ok(out)
}

/// How many of a selection carry each tag, and sit in each collector you
/// made. The tag popup draws "on every one" and "on some" from this, so the
/// arithmetic is done once, here, over the edges — not by the popup fetching
/// a record per item and counting.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectionTags {
    /// How many of the ids given still exist. The denominator for "every".
    pub total: usize,
    pub tags: HashMap<String, usize>,
    pub collectors: HashMap<String, usize>,
    /// The collectors you can gather into, whether or not any of the
    /// selection is in them — the popup offers all of them.
    pub targets: Vec<GatherTarget>,
}

pub fn selection_tags(conn: &Connection, ids: &[String]) -> Result<SelectionTags> {
    let mut tags: HashMap<String, usize> = HashMap::new();
    let mut collectors: HashMap<String, usize> = HashMap::new();
    let mut total = 0;
    for id in ids {
        let exists = conn
            .query_row("SELECT 1 FROM node WHERE id = ?1", params![id], |_| Ok(()))
            .optional()?
            .is_some();
        if !exists {
            continue;
        }
        total += 1;
        let mut q = conn.prepare(
            "SELECT target_id, kind FROM edge
              WHERE source_id = ?1 AND kind IN ('tag_of','contains') AND target_id IS NOT NULL",
        )?;
        let rows: Vec<(String, String)> = q
            .query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for (target, kind) in rows {
            let map = if kind == "tag_of" { &mut tags } else { &mut collectors };
            *map.entry(target).or_default() += 1;
        }
    }
    let targets = gather_targets(conn)?;
    // Membership in a mirrored folder is where the file is, not something the
    // popup can toggle — so it is not counted as if it were.
    collectors.retain(|id, _| targets.iter().any(|t| &t.id == id));
    Ok(SelectionTags {
        total,
        tags,
        collectors,
        targets,
    })
}

/* ------------------------------------------------------------- making */

/// What can be made from the interface. A tag is not here: tags are made by
/// naming one where it is applied, which is where the facet gets decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NewKind {
    Note,
    Folder,
    Board,
    Link,
}

impl NewKind {
    pub fn parse(s: &str) -> Result<NewKind> {
        Ok(match s {
            "note" => NewKind::Note,
            "folder" => NewKind::Folder,
            "board" => NewKind::Board,
            "link" => NewKind::Link,
            other => return Err(anyhow!("cannot make a {other}")),
        })
    }
}

pub const NOTE_TYPE: &str = "app.archiva.note.file";

/// Make one thing, optionally inside a collector, and return its row.
///
/// `name` is the note's title, the collector's name, or the link's title.
/// `url` is only read for a link. `into` must be a collector you made; one
/// that is not is refused before anything is written, rather than leaving a
/// new item floating outside the place it was asked for.
pub fn create(
    conn: &Connection,
    space_root: &Path,
    kind: NewKind,
    name: &str,
    url: Option<&str>,
    into: Option<&str>,
) -> Result<Row> {
    if let Some(c) = into {
        if gather_target(conn, c)?.is_none() {
            return Err(anyhow!("new things can only be made inside a collector made in Archiva"));
        }
    }
    let name = name.trim();

    let tx = conn.unchecked_transaction()?;
    let id = match kind {
        NewKind::Note => {
            let title = if name.is_empty() { "Untitled note" } else { name };
            let dir = space_root.join("notes");
            std::fs::create_dir_all(&dir)?;
            let path = unique_note_path(&dir, title);
            // The file first: a row pointing at a file that failed to write is
            // a note that says it exists and cannot be opened.
            std::fs::write(&path, format!("# {title}\n\n"))?;
            let id = uuid_v7();
            tx.execute(
                "INSERT INTO node(id, node_type, content_type, content_type_tree, title,
                                  source_kind, locator, parent_dir, filename, extension,
                                  availability, display_name, display_subtitle, icon_kind)
                 VALUES (?1, 'note', ?2, ?3, ?4, 'app_generated', ?5, ?6, ?7, 'md',
                         'present', ?4, 'note', ?8)",
                params![
                    id,
                    NOTE_TYPE,
                    serde_json::to_string(&content_type::closure(NOTE_TYPE))?,
                    title,
                    path.to_string_lossy(),
                    dir.to_string_lossy(),
                    path.file_name().map(|f| f.to_string_lossy().to_string()),
                    content_type::icon_kind(NOTE_TYPE),
                ],
            )?;
            tx.execute(
                "INSERT INTO note(node_id, storage, body) VALUES (?1, 'file', '')",
                params![id],
            )?;
            id
        }
        NewKind::Folder | NewKind::Board => {
            let (ct, collector_kind, fallback) = match kind {
                NewKind::Board => ("app.archiva.collector.board", "board", "Untitled board"),
                _ => ("app.archiva.collector.folder", "folder", "Untitled folder"),
            };
            let title = if name.is_empty() { fallback } else { name };
            let id = uuid_v7();
            tx.execute(
                "INSERT INTO node(id, node_type, content_type, content_type_tree, title,
                                  source_kind, display_name, display_subtitle, icon_kind)
                 VALUES (?1, 'collector', ?2, ?3, ?4, 'app_generated', ?4, ?5, ?6)",
                params![
                    id,
                    ct,
                    serde_json::to_string(&content_type::closure(ct))?,
                    title,
                    collector_kind,
                    content_type::icon_kind(ct),
                ],
            )?;
            tx.execute(
                "INSERT INTO collector(node_id, collector_kind) VALUES (?1, ?2)",
                params![id, collector_kind],
            )?;
            id
        }
        NewKind::Link => {
            let url = url.ok_or_else(|| anyhow!("a link needs a web address"))?;
            let title = if name.is_empty() { None } else { Some(name) };
            super::identity::add_remote(&tx, url, title)?
        }
    };
    if let Some(c) = into {
        mutations::link(&tx, &id, "N", c, None, None)?;
    }
    health::recompute(&tx, &id)?;
    tx.commit()?;
    projections::row(conn, &id)
}

/// `<title>.md`, then `<title> 2.md` and so on. Never overwrites: a note you
/// made yesterday with the same title is still yours.
fn unique_note_path(dir: &Path, title: &str) -> PathBuf {
    let stem: String = title
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .chars()
        .take(120)
        .collect();
    let stem = if stem.is_empty() { "Untitled note".to_string() } else { stem };
    let mut path = dir.join(format!("{stem}.md"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} {n}.md"));
        n += 1;
    }
    path
}

fn existing_ids(conn: &Connection, ids: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for id in ids {
        if conn
            .query_row("SELECT 1 FROM node WHERE id = ?1", params![id], |_| Ok(()))
            .optional()?
            .is_some()
        {
            out.push(id.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::db::migrate(&c).unwrap();
        c
    }

    fn media(c: &Connection, id: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,locator)
             VALUES (?1,'media','public.jpeg',?2,?1,'/p/' || ?1 || '.jpg')",
            params![id, serde_json::to_string(&content_type::closure("public.jpeg")).unwrap()],
        )
        .unwrap();
    }

    /// A folder mirrored from disk, the way `folders::rebuild` records one.
    fn mirrored(c: &Connection, id: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,
                              source_kind,locator)
             VALUES (?1,'collector','app.archiva.collector.folder','[]',?1,'app_generated','/p')",
            params![id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO collector(node_id,collector_kind) VALUES (?1,'folder')",
            params![id],
        )
        .unwrap();
    }

    fn kinds_from(c: &Connection, source: &str) -> Vec<(String, String)> {
        let mut q = c
            .prepare("SELECT target_id, kind FROM edge WHERE source_id = ?1 ORDER BY kind, target_id")
            .unwrap();
        q.query_map(params![source], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_arm_decides_the_kind_not_this_module() {
        // S6: the same gesture — put this in North — is a tagging for a tag,
        // membership for a collector, and a compass link for anything else.
        let c = db();
        media(&c, "a");
        media(&c, "b");
        let tag = crate::model::tags::ensure(&c, "harbour", "environment").unwrap();
        let board = create(&c, Path::new("/unused"), NewKind::Board, "Mood", None, None).unwrap();

        let out = add_to_arm(&c, "a", "N", &ids(&["b", &tag, &board.id])).unwrap();
        assert_eq!(out.created, 3);
        let mut got = kinds_from(&c, "a");
        got.sort();
        let mut want = vec![
            ("b".to_string(), "compass_n".to_string()),
            (tag.clone(), "tag_of".to_string()),
            (board.id.clone(), "contains".to_string()),
        ];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn adding_twice_is_counted_as_already_there() {
        let c = db();
        media(&c, "a");
        media(&c, "b");
        add_to_arm(&c, "a", "W", &ids(&["b"])).unwrap();
        let again = add_to_arm(&c, "a", "W", &ids(&["b"])).unwrap();
        assert_eq!(again, Added { created: 0, existed: 1, refused: vec![] });
        // West reads the same from both ends, so the far side asking is the
        // same claim, not a second one.
        let back = add_to_arm(&c, "b", "W", &ids(&["a"])).unwrap();
        assert_eq!(back.existed, 1);
    }

    #[test]
    fn one_bad_item_is_reported_without_losing_the_rest() {
        let c = db();
        media(&c, "a");
        media(&c, "b");
        let out = add_to_arm(&c, "a", "E", &ids(&["a", "b"])).unwrap();
        assert_eq!(out.created, 1);
        assert_eq!(out.refused.len(), 1);
        assert_eq!(out.refused[0].0, "a");
    }

    #[test]
    fn a_tag_put_in_north_changes_the_items_health() {
        let c = db();
        media(&c, "a");
        let tag = crate::model::tags::ensure(&c, "harbour", "environment").unwrap();
        add_to_arm(&c, "a", "N", &ids(&[&tag])).unwrap();
        let filled: i64 = c
            .query_row("SELECT facets_filled FROM node WHERE id='a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(filled, 1);

        let edge: String = c
            .query_row("SELECT id FROM edge WHERE source_id='a'", [], |r| r.get(0))
            .unwrap();
        unlink(&c, &edge).unwrap();
        let filled: i64 = c
            .query_row("SELECT facets_filled FROM node WHERE id='a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(filled, 0, "removing it from the arm takes the tag off");
    }

    #[test]
    fn a_folder_mirrored_from_disk_cannot_be_gathered_into() {
        let c = db();
        media(&c, "a");
        mirrored(&c, "disk");
        assert_eq!(gather_target(&c, "disk").unwrap(), None);
        assert!(gather(&c, &ids(&["a"]), "disk").is_err());
        assert!(kinds_from(&c, "a").is_empty(), "nothing written on the way to refusing");
    }

    #[test]
    fn a_collector_made_here_gathers_and_lets_go() {
        let c = db();
        media(&c, "a");
        media(&c, "b");
        let folder = create(&c, Path::new("/unused"), NewKind::Folder, "Picks", None, None).unwrap();
        let t = gather_target(&c, &folder.id).unwrap().unwrap();
        assert_eq!((t.name.as_str(), t.kind.as_str()), ("Picks", "folder"));

        let out = gather(&c, &ids(&["a", "b"]), &folder.id).unwrap();
        assert_eq!(out.created, 2);
        assert_eq!(kinds_from(&c, "a"), vec![(folder.id.clone(), "contains".to_string())]);
        assert_eq!(ungather(&c, &ids(&["a", "b"]), &folder.id).unwrap(), 2);
        assert!(kinds_from(&c, "a").is_empty());
    }

    #[test]
    fn selection_tags_counts_every_and_some() {
        let c = db();
        media(&c, "a");
        media(&c, "b");
        mirrored(&c, "disk");
        let harbour = crate::model::tags::ensure(&c, "harbour", "environment").unwrap();
        let boats = crate::model::tags::ensure(&c, "boats", "subject").unwrap();
        crate::model::tags::apply(&c, &ids(&["a", "b"]), &harbour).unwrap();
        crate::model::tags::apply(&c, &ids(&["a"]), &boats).unwrap();
        let folder = create(&c, Path::new("/unused"), NewKind::Folder, "Picks", None, None).unwrap();
        gather(&c, &ids(&["b"]), &folder.id).unwrap();
        // Membership of the mirrored folder, the way the scan writes it.
        c.execute(
            "INSERT INTO edge(id,source_id,target_id,kind) VALUES ('m','a','disk','contains')",
            [],
        )
        .unwrap();

        let s = selection_tags(&c, &ids(&["a", "b", "gone"])).unwrap();
        assert_eq!(s.total, 2, "an id that no longer exists is not in the denominator");
        assert_eq!(s.tags.get(&harbour), Some(&2));
        assert_eq!(s.tags.get(&boats), Some(&1));
        assert_eq!(s.collectors.get(&folder.id), Some(&1));
        assert!(!s.collectors.contains_key("disk"), "where a file sits is not a toggle");
        assert_eq!(s.targets.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec![folder.id.as_str()]);
    }

    #[test]
    fn a_new_note_is_a_real_file_in_the_space() {
        let c = db();
        let space = std::env::temp_dir().join(format!("archiva-relate-{}", uuid_v7()));
        let row = create(&c, &space, NewKind::Note, "Harbour ideas", None, None).unwrap();
        assert_eq!(row.node_type, "note");
        assert_eq!(row.display_name, "Harbour ideas");

        let path = space.join("notes").join("Harbour ideas.md");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Harbour ideas\n\n");
        let body = crate::model::notetext::body(&c, &row.id).unwrap().unwrap();
        assert_eq!(body.storage, "file");
        assert!(body.text.starts_with("# Harbour ideas"));
        assert!(row.capabilities.iter().any(|x| x == "edit"), "{:?}", row.capabilities);

        // The same title again is a second note, not the first one overwritten.
        let again = create(&c, &space, NewKind::Note, "Harbour ideas", None, None).unwrap();
        assert_ne!(again.id, row.id);
        assert!(space.join("notes").join("Harbour ideas 2.md").exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Harbour ideas\n\n");

        // Found by name, through the index a scan would have written.
        let hits = crate::model::search::search(&c, "harbour", &Default::default()).unwrap();
        assert!(hits.iter().any(|h| h.node.id == row.id));
        std::fs::remove_dir_all(&space).ok();
    }

    #[test]
    fn a_new_note_survives_the_scan_that_cannot_see_it() {
        // The space is never walked (invariant 9). A note recorded as a local
        // file would be marked missing by the very next Refresh.
        let mut c = db();
        let space = std::env::temp_dir().join(format!("archiva-relate-{}", uuid_v7()));
        let row = create(&c, &space, NewKind::Note, "Kept", None, None).unwrap();
        struct Bare;
        impl crate::model::scan::Extractor for Bare {
            fn extract(&self, _: &Path, _: &str) -> Vec<(String, Option<String>, Option<f64>)> {
                vec![]
            }
            fn version(&self) -> i64 {
                1
            }
            fn proxies(&self, _: &Path, _: &str, _: Option<&str>) -> crate::model::scan::Proxies {
                crate::model::scan::Proxies::not_applicable(1)
            }
        }
        let empty = std::env::temp_dir().join(format!("archiva-relate-empty-{}", uuid_v7()));
        std::fs::create_dir_all(&empty).unwrap();
        crate::model::scan::scan(&mut c, &[empty.clone()], &[space.clone()], &Bare).unwrap();
        let availability: String = c
            .query_row("SELECT availability FROM node WHERE id = ?1", params![row.id], |r| r.get(0))
            .unwrap();
        assert_eq!(availability, "present");
        std::fs::remove_dir_all(&space).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn made_inside_a_collector_is_in_it() {
        let c = db();
        let board = create(&c, Path::new("/unused"), NewKind::Board, "Mood", None, None).unwrap();
        let link = create(
            &c,
            Path::new("/unused"),
            NewKind::Link,
            "",
            Some("https://example.com/pic.jpg"),
            Some(&board.id),
        )
        .unwrap();
        assert_eq!(link.availability, "remote_uncached");
        assert_eq!(kinds_from(&c, &link.id), vec![(board.id.clone(), "contains".to_string())]);
    }

    #[test]
    fn asking_to_make_something_inside_a_mirrored_folder_writes_nothing() {
        let c = db();
        mirrored(&c, "disk");
        let before: i64 = c.query_row("SELECT COUNT(*) FROM node", [], |r| r.get(0)).unwrap();
        assert!(create(&c, Path::new("/unused"), NewKind::Folder, "x", None, Some("disk")).is_err());
        let after: i64 = c.query_row("SELECT COUNT(*) FROM node", [], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn rows_of_keeps_the_order_asked_and_skips_what_is_gone() {
        let c = db();
        media(&c, "a");
        media(&c, "b");
        let rows = rows_of(&c, &ids(&["b", "gone", "a"])).unwrap();
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["b", "a"]);
    }
}
