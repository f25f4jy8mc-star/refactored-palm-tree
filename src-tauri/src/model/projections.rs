//! `p_detail` — everything about one node, in one call.
//!
//! What Compass and the Inspector both read. Two rules from the model matter
//! here and are implemented in exactly one place each:
//!
//!   * **Compass is derived from kind** (§1.7). Nothing stores a direction, so
//!     tagging and collector membership can read as North together while
//!     staying distinct underneath.
//!   * **North and South invert; West and East do not** (G23). Broader and
//!     narrower are converse relations; related and opposing are symmetric.
//!     Reading an East link back as West would quietly weaken a claim the user
//!     made.
//!
//! Every link entry carries the far node's row (G22), so drawing a compass is
//! one query rather than one query per tile.
//!
//! Suggested edges are excluded unless asked for (G8) — principle 3: nothing
//! the machine proposed is ever shown as something you said.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;

use super::capabilities::{self, Subject};
use super::content_type;

/// The read rule. One place, used for reading and (inverted) for writing.
pub fn compass_of(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "tag_of" | "contains" | "compass_n" => "N",
        "compass_s" => "S",
        "compass_w" | "wikilink" | "embed" => "W",
        "compass_e" => "E",
        _ => return None, // board_position has no direction
    })
}

/// G23. The correction the Compass view forced.
pub fn reciprocal(compass: &str) -> &'static str {
    match compass {
        "N" => "S",
        "S" => "N",
        "W" => "W",
        "E" => "E",
        _ => "N",
    }
}

/// The write rule (G21): the drop zone decides the kind, so an item cannot
/// land in the tags panel and quietly become something else.
pub fn kind_for_drop(compass: &str, target_node_type: &str) -> &'static str {
    match compass {
        "N" => match target_node_type {
            "tag" => "tag_of",
            "collector" => "contains",
            _ => "compass_n",
        },
        "S" => "compass_s",
        "W" => "compass_w",
        "E" => "compass_e",
        _ => "compass_n",
    }
}

/// Ordering inside a slot: by target type first, then by ordinal within the
/// type (G25). Type, not edge kind — East holds one kind but often several
/// types, and the split has to fall where the user can see it.
fn type_rank(node_type: &str, content_type: &str) -> u8 {
    match node_type {
        "tag" => 0,
        "collector" => 1,
        "note" => 7,
        _ => {
            let tree = content_type::closure(content_type);
            let has = |t: &str| tree.iter().any(|x| x == t);
            if has("public.image") {
                2
            } else if has("public.composite-content") {
                3
            } else if has("public.movie") {
                4
            } else if has("public.audio") {
                5
            } else {
                6
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub id: String,
    pub node_type: String,
    pub content_type: String,
    pub display_name: String,
    pub display_subtitle: String,
    pub icon_kind: String,
    pub availability: String,
    pub proxy_state: String,
    pub thumb_ref: Option<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Link {
    pub edge_id: String,
    pub kind: String,
    pub compass: String,
    /// What this item looks like from the far node's side.
    pub reciprocal: String,
    pub ordinal: i64,
    pub label: Option<String>,
    pub status: String,
    pub origin: String,
    /// True when the edge is stored pointing away from this node. Views do not
    /// need it; it is here so a bug in direction handling is visible.
    pub outward: bool,
    pub node: Row,
}

#[derive(Debug, Serialize)]
pub struct Slot {
    pub compass: String,
    /// Total in this direction, before any cap. The view must be able to say
    /// how much it is hiding without fetching everything (G25).
    pub total: usize,
    pub groups: Vec<Group>,
}

#[derive(Debug, Serialize)]
pub struct Group {
    pub node_type: String,
    pub total: usize,
    pub links: Vec<Link>,
}

#[derive(Debug, Serialize)]
pub struct Detail {
    pub node: Row,
    pub attributes: BTreeMap<String, String>,
    pub slots: Vec<Slot>,
    pub suggestions: Vec<Link>,
    pub unresolved_links: i64,
}

pub struct Options {
    /// Tiles per type group before the view collapses the rest behind a "+n".
    pub cap_per_group: usize,
    /// Suggestions are returned separately, never mixed into the slots.
    pub include_suggested: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            cap_per_group: 4,
            include_suggested: true,
        }
    }
}

pub fn detail(conn: &Connection, id: &str, opts: &Options) -> Result<Detail> {
    let node = row(conn, id)?;
    let attributes = attributes(conn, id)?;

    let mut declared: Vec<Link> = Vec::new();
    let mut suggestions: Vec<Link> = Vec::new();

    // Both directions in one pass. An edge is stored once; which way it reads
    // depends on which end you are standing at.
    let sql = "
        SELECT e.id, e.kind, e.ordinal, e.label, e.status, e.origin, 1 AS outward, e.target_id
          FROM edge e
         WHERE e.source_id = ?1 AND e.target_id IS NOT NULL AND e.kind <> 'board_position'
        UNION ALL
        SELECT e.id, e.kind, e.ordinal, e.label, e.status, e.origin, 0 AS outward, e.source_id
          FROM edge e
         WHERE e.target_id = ?1 AND e.kind <> 'board_position'";

    let mut q = conn.prepare(sql)?;
    let raw: Vec<(String, String, i64, Option<String>, String, String, i64, String)> = q
        .query_map(params![id], |r| {
            Ok((
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
                r.get(7)?,
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;

    for (edge_id, kind, ordinal, label, status, origin, outward, other) in raw {
        let Some(base) = compass_of(&kind) else { continue };
        let outward = outward == 1;
        // The whole of G23 lives on this line.
        let compass = if outward { base } else { reciprocal(base) };
        let link = Link {
            edge_id,
            kind,
            compass: compass.to_string(),
            reciprocal: reciprocal(compass).to_string(),
            ordinal,
            label,
            status: status.clone(),
            origin,
            outward,
            node: row(conn, &other)?,
        };
        if status == "suggested" {
            suggestions.push(link);
        } else {
            declared.push(link);
        }
    }

    if !opts.include_suggested {
        suggestions.clear();
    }

    let slots = ["N", "S", "W", "E"]
        .iter()
        .map(|c| build_slot(c, &declared, opts.cap_per_group))
        .collect();

    let unresolved_links: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edge WHERE source_id = ?1 AND target_id IS NULL",
        params![id],
        |r| r.get(0),
    )?;

    Ok(Detail {
        node,
        attributes,
        slots,
        suggestions,
        unresolved_links,
    })
}

fn build_slot(compass: &str, links: &[Link], cap: usize) -> Slot {
    let mut mine: Vec<&Link> = links.iter().filter(|l| l.compass == compass).collect();

    // West and East are symmetric (G23), so A→B and B→A assert the same thing.
    // v1's interface presented the two directions separately, so real libraries
    // contain both, and without this they render as two identical tiles.
    //
    // The outward edge wins: it is the one the user created from this side, so
    // it carries the label they wrote here. North and South are converse rather
    // than symmetric, so a pair there is two genuinely different claims and is
    // left alone.
    if compass == "W" || compass == "E" {
        let mut seen: Vec<&str> = Vec::new();
        mine.sort_by_key(|l| !l.outward); // outward first
        mine.retain(|l| {
            if seen.contains(&l.node.id.as_str()) {
                false
            } else {
                seen.push(&l.node.id);
                true
            }
        });
    }

    mine.sort_by_key(|l| {
        (
            type_rank(&l.node.node_type, &l.node.content_type),
            l.ordinal,
            l.node.display_name.clone(),
        )
    });

    let total = mine.len();
    let mut groups: Vec<Group> = Vec::new();
    for l in mine {
        let key = l.node.node_type.clone();
        match groups.last_mut() {
            Some(g) if g.node_type == key => {
                g.total += 1;
                if g.links.len() < cap {
                    g.links.push(clone_link(l));
                }
            }
            _ => groups.push(Group {
                node_type: key,
                total: 1,
                links: vec![clone_link(l)],
            }),
        }
    }

    Slot {
        compass: compass.to_string(),
        total,
        groups,
    }
}

fn clone_link(l: &Link) -> Link {
    Link {
        edge_id: l.edge_id.clone(),
        kind: l.kind.clone(),
        compass: l.compass.clone(),
        reciprocal: l.reciprocal.clone(),
        ordinal: l.ordinal,
        label: l.label.clone(),
        status: l.status.clone(),
        origin: l.origin.clone(),
        outward: l.outward,
        node: Row {
            id: l.node.id.clone(),
            node_type: l.node.node_type.clone(),
            content_type: l.node.content_type.clone(),
            display_name: l.node.display_name.clone(),
            display_subtitle: l.node.display_subtitle.clone(),
            icon_kind: l.node.icon_kind.clone(),
            availability: l.node.availability.clone(),
            proxy_state: l.node.proxy_state.clone(),
            thumb_ref: l.node.thumb_ref.clone(),
            capabilities: l.node.capabilities.clone(),
        },
    }
}

fn row(conn: &Connection, id: &str) -> Result<Row> {
    let (
        node_type,
        content_type,
        display_name,
        display_subtitle,
        icon_kind,
        availability,
        proxy_state,
        thumb_ref,
        playable,
        source_kind,
    ): (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
    ) = conn.query_row(
        "SELECT node_type, content_type, display_name, display_subtitle, icon_kind,
                availability, proxy_state, proxy_thumb_ref, proxy_playable_ref, source_kind
           FROM node WHERE id = ?1",
        params![id],
        |r| {
            Ok((
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
                r.get(7)?, r.get(8)?, r.get(9)?,
            ))
        },
    )?;

    // Capabilities are computed, never stored (G15). The extra fields below
    // are the instance conditions — the difference between "could" and
    // "can right now".
    let note_storage: Option<String> = conn
        .query_row("SELECT storage FROM note WHERE node_id = ?1", params![id], |r| r.get(0))
        .ok();
    let tag_facet: Option<String> = conn
        .query_row("SELECT facet FROM tag WHERE node_id = ?1", params![id], |r| r.get(0))
        .ok();
    let item_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edge WHERE target_id = ?1 AND kind = 'contains'",
        params![id],
        |r| r.get(0),
    )?;
    let duration: Option<f64> = conn
        .query_row(
            "SELECT value_num FROM attribute WHERE node_id = ?1 AND key = 'duration'",
            params![id],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    let page_count: Option<i64> = conn
        .query_row(
            "SELECT value_num FROM attribute WHERE node_id = ?1 AND key = 'page_count'",
            params![id],
            |r| r.get::<_, Option<f64>>(0),
        )
        .ok()
        .flatten()
        .map(|v| v as i64);

    let subject = Subject {
        content_type: content_type.clone(),
        source_kind: source_kind.clone(),
        availability: availability.clone(),
        proxy_state: proxy_state.clone(),
        proxy_playable: playable.is_some(),
        writable: availability == "present",
        note_storage,
        tag_facet,
        item_count,
        duration,
        page_count,
        codec_native: true,
    };

    Ok(Row {
        id: id.to_string(),
        node_type,
        content_type,
        display_name,
        display_subtitle,
        icon_kind,
        availability,
        proxy_state,
        thumb_ref,
        capabilities: capabilities::capabilities_of(&subject),
    })
}

fn attributes(conn: &Connection, id: &str) -> Result<BTreeMap<String, String>> {
    let mut q = conn.prepare("SELECT key, value FROM attribute WHERE node_id = ?1 ORDER BY key")?;
    let rows = q.query_map(params![id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        let (k, v) = row?;
        out.insert(k, v.unwrap_or_default());
    }
    Ok(out)
}


/* ==========================================================================
   p_rows — the projection every list reads.
   ========================================================================== */

/// Rows come back **already grouped, sorted and flattened**, with a contiguous
/// `ordinal` (G11). That is the whole point: the screen draws in ordinal order
/// and the arrow keys walk in ordinal order, so the two cannot disagree. Build
/// 17 derives them separately, which is the arrow-key bug.
///
/// `expanded` is an argument rather than view state (G12), so a collector's
/// children arrive in the same flat list with a `depth`, and no view keeps a
/// private cache of index data.
#[derive(Debug, Serialize)]
pub struct ListRow {
    pub id: String,
    pub node_type: String,
    pub content_type: String,
    pub display_name: String,
    pub display_subtitle: String,
    pub icon_kind: String,
    pub availability: String,
    pub proxy_state: String,
    pub thumb_ref: Option<String>,
    pub size_bytes: Option<i64>,
    pub indexed_at: String,
    pub captured_at: Option<String>,
    pub health: i64,
    /// Which of the three signals are missing, so the hint comes from data
    /// rather than a switch in the view (G20).
    pub health_missing: Vec<String>,
    pub capabilities: Vec<String>,
    pub group_key: String,
    pub group_label: String,
    pub depth: i64,
    pub ordinal: i64,
    pub child_count: i64,
}

#[derive(Debug, Serialize)]
pub struct ListPage {
    pub rows: Vec<ListRow>,
    pub total: usize,
    pub group_by: String,
    pub sort: String,
}

pub struct ListOptions {
    /// A collector id, or None for the whole library.
    pub scope: Option<String>,
    /// `type` · `health` · `month` · `none`
    pub group_by: String,
    /// `name` · `date` · `captured` · `size` · `health`
    pub sort: String,
    pub descending: bool,
    /// Collector ids whose children should be inlined beneath them.
    pub expanded: Vec<String>,
    /// Free text over the display name.
    pub query: Option<String>,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            scope: None,
            group_by: "type".into(),
            sort: "name".into(),
            descending: false,
            expanded: Vec::new(),
            query: None,
        }
    }
}

const HEALTH_LABEL: [&str; 4] = [
    "Not described",
    "Barely described",
    "Hard to search by name",
    "Well described",
];

pub fn rows(conn: &Connection, opts: &ListOptions) -> Result<ListPage> {
    let mut flat: Vec<ListRow> = Vec::new();
    let top = fetch(conn, opts.scope.as_deref(), opts)?;

    // Group, then sort within the group, then flatten. Doing it in that order
    // here — once — is what makes the rendered order and the keyboard order the
    // same sequence.
    let mut grouped: Vec<(String, String, Vec<Raw>)> = Vec::new();
    for r in top {
        let (key, label) = group_of(&r, &opts.group_by);
        match grouped.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, _, v)) => v.push(r),
            None => grouped.push((key, label, vec![r])),
        }
    }
    grouped.sort_by(|a, b| group_rank(&a.0, &opts.group_by).cmp(&group_rank(&b.0, &opts.group_by)));

    for (key, label, mut members) in grouped {
        sort_rows(&mut members, &opts.sort, opts.descending);
        for m in members {
            let expanded = opts.expanded.iter().any(|e| *e == m.id);
            let children = if expanded {
                let mut kids = fetch(conn, Some(&m.id), opts)?;
                sort_rows(&mut kids, &opts.sort, opts.descending);
                kids
            } else {
                Vec::new()
            };
            flat.push(to_row(conn, m, &key, &label, 0, flat.len() as i64)?);
            for c in children {
                let n = flat.len() as i64;
                flat.push(to_row(conn, c, &key, &label, 1, n)?);
            }
        }
    }

    Ok(ListPage {
        total: flat.len(),
        rows: flat,
        group_by: opts.group_by.clone(),
        sort: opts.sort.clone(),
    })
}

struct Raw {
    id: String,
    node_type: String,
    content_type: String,
    display_name: String,
    display_subtitle: String,
    icon_kind: String,
    availability: String,
    proxy_state: String,
    thumb_ref: Option<String>,
    size_bytes: Option<i64>,
    indexed_at: String,
    health: i64,
    facets_filled: i64,
    title_quality: i64,
    has_any_tag: i64,
    captured_at: Option<String>,
}

fn fetch(conn: &Connection, scope: Option<&str>, opts: &ListOptions) -> Result<Vec<Raw>> {
    // A scoped list is the members of one collector; unscoped is everything
    // except the virtual nodes, which have their own views.
    let sql = match scope {
        Some(_) => {
            "SELECT n.id, n.node_type, n.content_type, n.display_name, n.display_subtitle,
                    n.icon_kind, n.availability, n.proxy_state, n.proxy_thumb_ref,
                    n.size_bytes, n.indexed_at, n.tagging_health, n.facets_filled,
                    n.title_quality, n.has_any_tag,
                    (SELECT value FROM attribute a WHERE a.node_id = n.id AND a.key = 'captured_at')
               FROM node n
               JOIN edge e ON e.source_id = n.id AND e.kind = 'contains' AND e.target_id = ?1
              WHERE (?2 IS NULL OR n.display_name LIKE ?2)"
        }
        None => {
            "SELECT n.id, n.node_type, n.content_type, n.display_name, n.display_subtitle,
                    n.icon_kind, n.availability, n.proxy_state, n.proxy_thumb_ref,
                    n.size_bytes, n.indexed_at, n.tagging_health, n.facets_filled,
                    n.title_quality, n.has_any_tag,
                    (SELECT value FROM attribute a WHERE a.node_id = n.id AND a.key = 'captured_at')
               FROM node n
              WHERE n.node_type <> 'tag'
                AND (?2 IS NULL OR n.display_name LIKE ?2)
                AND ?1 IS NULL"
        }
    };
    let like = opts.query.as_ref().map(|q| format!("%{q}%"));
    let mut q = conn.prepare(sql)?;
    let out = q
        .query_map(params![scope, like], |r| {
            Ok(Raw {
                id: r.get(0)?,
                node_type: r.get(1)?,
                content_type: r.get(2)?,
                display_name: r.get(3)?,
                display_subtitle: r.get(4)?,
                icon_kind: r.get(5)?,
                availability: r.get(6)?,
                proxy_state: r.get(7)?,
                thumb_ref: r.get(8)?,
                size_bytes: r.get(9)?,
                indexed_at: r.get(10)?,
                health: r.get(11)?,
                facets_filled: r.get(12)?,
                title_quality: r.get(13)?,
                has_any_tag: r.get(14)?,
                captured_at: r.get(15)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(out)
}

fn group_of(r: &Raw, by: &str) -> (String, String) {
    match by {
        "health" => (
            r.health.to_string(),
            HEALTH_LABEL[r.health.clamp(0, 3) as usize].to_string(),
        ),
        "month" => {
            // Date added, not date taken. Which axis a gallery groups by is a
            // choice the caller makes, and the two are genuinely different.
            let m = r.indexed_at.get(0..7).unwrap_or("unknown").to_string();
            (m.clone(), m)
        }
        "none" => ("all".into(), String::new()),
        _ => {
            let k = type_group(&r.node_type, &r.content_type);
            (k.0.to_string(), k.1.to_string())
        }
    }
}

fn type_group(node_type: &str, content_type: &str) -> (&'static str, &'static str) {
    if node_type == "collector" {
        return ("collector", "Collectors");
    }
    if node_type == "note" {
        return ("note", "Notes");
    }
    let tree = content_type::closure(content_type);
    let has = |t: &str| tree.iter().any(|x| x == t);
    // Conformance queries, not string comparison — a HEIC is an image without
    // anyone editing a list (G17).
    if has("public.image") {
        ("image", "Images")
    } else if has("public.movie") {
        ("video", "Video")
    } else if has("public.audio") {
        ("audio", "Audio")
    } else if has("public.composite-content") {
        ("document", "Documents")
    } else if has("public.3d-content") {
        ("model", "3D")
    } else {
        ("other", "Other")
    }
}

fn group_rank(key: &str, by: &str) -> (i64, String) {
    match by {
        // Worst-described first: the point of the Scattered view is to surface
        // what needs attention, not to bury it.
        "health" => (key.parse::<i64>().unwrap_or(9), String::new()),
        "month" => (0, key.to_string()),
        "none" => (0, String::new()),
        _ => {
            let order = ["image", "video", "audio", "document", "model", "note", "collector", "other"];
            (
                order.iter().position(|k| *k == key).unwrap_or(99) as i64,
                String::new(),
            )
        }
    }
}

fn sort_rows(rows: &mut [Raw], by: &str, desc: bool) {
    match by {
        "date" => rows.sort_by(|a, b| a.indexed_at.cmp(&b.indexed_at)),
        "captured" => rows.sort_by(|a, b| {
            // Nothing without a capture date sinks to the bottom rather than
            // sorting as if it were taken in 1970.
            match (&a.captured_at, &b.captured_at) {
                (Some(x), Some(y)) => x.cmp(y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.display_name.cmp(&b.display_name),
            }
        }),
        "size" => rows.sort_by_key(|r| r.size_bytes.unwrap_or(0)),
        "health" => rows.sort_by_key(|r| r.health),
        _ => rows.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
                // Ties break by id, and ids are UUID v7, so the order is stable
                // across calls rather than whatever SQLite happened to return.
                .then(a.id.cmp(&b.id))
        }),
    }
    if desc {
        rows.reverse();
    }
}

fn to_row(
    conn: &Connection,
    r: Raw,
    group_key: &str,
    group_label: &str,
    depth: i64,
    ordinal: i64,
) -> Result<ListRow> {
    let mut missing = Vec::new();
    if r.has_any_tag == 0 {
        missing.push("no tags".to_string());
    } else if r.facets_filled < 3 {
        missing.push(format!("{} of 3 facets", r.facets_filled));
    }
    if r.title_quality == 0 {
        missing.push("filename as title".to_string());
    }

    let child_count: i64 = if r.node_type == "collector" {
        conn.query_row(
            "SELECT COUNT(*) FROM edge WHERE target_id = ?1 AND kind = 'contains'",
            params![r.id],
            |x| x.get(0),
        )?
    } else {
        0
    };

    let full = row(conn, &r.id)?;
    Ok(ListRow {
        id: r.id,
        node_type: r.node_type,
        content_type: r.content_type,
        display_name: r.display_name,
        display_subtitle: r.display_subtitle,
        icon_kind: r.icon_kind,
        availability: r.availability,
        proxy_state: r.proxy_state,
        thumb_ref: r.thumb_ref,
        size_bytes: r.size_bytes,
        indexed_at: r.indexed_at,
        captured_at: r.captured_at,
        health: r.health,
        health_missing: missing,
        capabilities: full.capabilities,
        group_key: group_key.to_string(),
        group_label: group_label.to_string(),
        depth,
        ordinal,
        child_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagging_and_membership_both_read_as_north() {
        assert_eq!(compass_of("tag_of"), Some("N"));
        assert_eq!(compass_of("contains"), Some("N"));
        assert_eq!(compass_of("compass_n"), Some("N"));
        assert_eq!(compass_of("board_position"), None);
    }

    /// G23. Vertical directions invert; lateral ones do not.
    #[test]
    fn north_and_south_invert_but_west_and_east_do_not() {
        assert_eq!(reciprocal("N"), "S");
        assert_eq!(reciprocal("S"), "N");
        assert_eq!(reciprocal("W"), "W");
        assert_eq!(reciprocal("E"), "E");
    }

    /// The read rule and the write rule must be exact inverses, or a link
    /// created by dragging would come back as something else.
    #[test]
    fn the_write_rule_inverts_the_read_rule() {
        for (compass, target) in [
            ("N", "tag"),
            ("N", "collector"),
            ("N", "media"),
            ("S", "media"),
            ("W", "note"),
            ("E", "media"),
        ] {
            let kind = kind_for_drop(compass, target);
            assert_eq!(
                compass_of(kind),
                Some(compass),
                "dropping a {target} on {compass} produced {kind}, which reads back wrong"
            );
        }
    }

    #[test]
    fn slots_order_tags_then_collectors_then_media_then_notes() {
        assert!(type_rank("tag", "app.archiva.tag") < type_rank("collector", "app.archiva.collector.folder"));
        assert!(type_rank("collector", "app.archiva.collector.folder") < type_rank("media", "public.jpeg"));
        assert!(type_rank("media", "public.jpeg") < type_rank("media", "com.adobe.pdf"));
        assert!(type_rank("media", "com.adobe.pdf") < type_rank("note", "app.archiva.note.file"));
    }

    fn link(compass: &str, id: &str, outward: bool, node_type: &str) -> Link {
        Link {
            edge_id: format!("e-{id}-{outward}"),
            kind: format!("compass_{}", compass.to_lowercase()),
            compass: compass.into(),
            reciprocal: reciprocal(compass).into(),
            ordinal: 0,
            label: None,
            status: "declared".into(),
            origin: "user".into(),
            outward,
            node: Row {
                id: id.into(),
                node_type: node_type.into(),
                content_type: "public.jpeg".into(),
                display_name: id.into(),
                display_subtitle: String::new(),
                icon_kind: "image".into(),
                availability: "present".into(),
                proxy_state: "ready".into(),
                thumb_ref: None,
                capabilities: vec![],
            },
        }
    }

    /// v1 let you assert both directions of a lateral link, and its interface
    /// showed them separately. Under the corrected symmetric rule both now read
    /// as West from the same side, so without collapsing them the same item
    /// appears twice in one slot.
    #[test]
    fn a_doubly_asserted_lateral_link_renders_once() {
        let links = vec![
            link("W", "turin", true, "media"),
            link("W", "turin", false, "media"),
        ];
        let slot = build_slot("W", &links, 4);
        assert_eq!(slot.total, 1);
        assert!(slot.groups[0].links[0].outward, "the edge written from this side wins");
    }

    /// North and South are converse, not symmetric, so a pair there is two
    /// genuinely different claims and must survive.
    #[test]
    fn a_converse_vertical_pair_is_not_collapsed() {
        let links = vec![
            link("N", "a", true, "media"),
            link("N", "b", false, "media"),
        ];
        assert_eq!(build_slot("N", &links, 4).total, 2);
    }

    #[test]
    fn a_group_reports_its_total_even_when_capped() {
        let links: Vec<Link> = (0..7)
            .map(|i| link("E", &format!("n{i}"), true, "media"))
            .collect();
        let slot = build_slot("E", &links, 4);
        assert_eq!(slot.total, 7);
        assert_eq!(slot.groups[0].total, 7, "the view must know what it is hiding");
        assert_eq!(slot.groups[0].links.len(), 4);
    }

    fn seed() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql")).unwrap();
        let mk = |id: &str, nt: &str, ct: &str, name: &str, health: i64| {
            c.execute(
                "INSERT INTO node(id,node_type,content_type,content_type_tree,display_name,
                                  icon_kind,tagging_health,facets_filled,has_any_tag,indexed_at)
                 VALUES (?1,?2,?3,'[]',?4,'x',?5,?6,?7,'2026-01-01')",
                params![id, nt, ct, name, health, if health > 1 { 3 } else { 0 }, i64::from(health > 0)],
            )
            .unwrap();
        };
        mk("n1", "media", "public.jpeg", "zebra", 3);
        mk("n2", "media", "public.jpeg", "Apple", 0);
        mk("n3", "media", "com.adobe.pdf", "sheet", 1);
        mk("n4", "note", "app.archiva.note.file", "notes", 2);
        mk("k1", "collector", "app.archiva.collector.folder", "Folder", 3);
        c.execute("INSERT INTO collector(node_id,collector_kind) VALUES('k1','folder')", []).unwrap();
        c.execute(
            "INSERT INTO edge(id,source_id,target_id,kind) VALUES('e1','n1','k1','contains')",
            [],
        )
        .unwrap();
        c
    }

    /// The fix for the arrow-key bug: one sequence, produced once.
    #[test]
    fn ordinals_are_contiguous_and_match_the_rendered_order() {
        let c = seed();
        let page = rows(&c, &ListOptions::default()).unwrap();
        let ords: Vec<i64> = page.rows.iter().map(|r| r.ordinal).collect();
        assert_eq!(ords, (0..page.rows.len() as i64).collect::<Vec<_>>());
        assert_eq!(page.total, page.rows.len());
    }

    #[test]
    fn grouping_precedes_sorting_within_the_group() {
        let c = seed();
        let page = rows(&c, &ListOptions::default()).unwrap();
        let groups: Vec<&str> = page.rows.iter().map(|r| r.group_key.as_str()).collect();
        // Images, then documents, then notes, then collectors — never interleaved.
        let mut seen = Vec::new();
        for g in &groups {
            if seen.last() != Some(g) {
                assert!(!seen.contains(g), "group {g} appeared twice");
                seen.push(*g);
            }
        }
        assert_eq!(seen, vec!["image", "document", "note", "collector"]);
    }

    #[test]
    fn name_sorting_ignores_case() {
        let c = seed();
        let page = rows(&c, &ListOptions::default()).unwrap();
        let images: Vec<&str> = page
            .rows
            .iter()
            .filter(|r| r.group_key == "image")
            .map(|r| r.display_name.as_str())
            .collect();
        assert_eq!(images, vec!["Apple", "zebra"]);
    }

    /// Expansion is an argument, not view state (G12), and children arrive in
    /// the same flat list with a depth.
    #[test]
    fn expanding_a_collector_inlines_its_children() {
        let c = seed();
        let mut o = ListOptions::default();
        assert!(rows(&c, &o).unwrap().rows.iter().all(|r| r.depth == 0));
        o.expanded = vec!["k1".into()];
        let page = rows(&c, &o).unwrap();
        let child = page.rows.iter().find(|r| r.depth == 1).expect("no child row");
        assert_eq!(child.id, "n1");
        let ords: Vec<i64> = page.rows.iter().map(|r| r.ordinal).collect();
        assert_eq!(ords, (0..page.rows.len() as i64).collect::<Vec<_>>(),
                   "ordinals stay contiguous through an expansion");
    }

    /// Worst-described first — the point of Scattered is to surface what needs
    /// attention, not bury it.
    #[test]
    fn health_grouping_puts_the_worst_first() {
        let c = seed();
        let o = ListOptions { group_by: "health".into(), ..Default::default() };
        let page = rows(&c, &o).unwrap();
        assert_eq!(page.rows[0].health, 0);
        assert_eq!(page.rows[0].group_label, "Not described");
        assert!(page.rows[0].health_missing.iter().any(|m| m == "no tags"));
    }

    /// Type grouping is a conformance query, so a HEIC is an image without
    /// anyone editing a list (G17).
    #[test]
    fn type_groups_come_from_conformance_not_string_matching() {
        assert_eq!(type_group("media", "public.heic").0, "image");
        assert_eq!(type_group("media", "com.adobe.photoshop-image").0, "image");
        assert_eq!(type_group("media", "com.adobe.pdf").0, "document");
        assert_eq!(type_group("collector", "app.archiva.collector.board").0, "collector");
    }

    #[test]
    fn a_scoped_list_returns_only_that_collectors_members() {
        let c = seed();
        let o = ListOptions { scope: Some("k1".into()), ..Default::default() };
        let page = rows(&c, &o).unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].id, "n1");
    }

    #[test]
    fn items_with_no_capture_date_sink_rather_than_sorting_as_1970() {
        let c = seed();
        c.execute(
            "INSERT INTO attribute(node_id,key,value) VALUES('n2','captured_at','2020-01-01 00:00:00')",
            [],
        )
        .unwrap();
        let o = ListOptions { sort: "captured".into(), group_by: "none".into(), ..Default::default() };
        let page = rows(&c, &o).unwrap();
        assert_eq!(page.rows[0].id, "n2", "the one with a date leads");
        assert!(page.rows[1..].iter().all(|r| r.captured_at.is_none()));
    }
}
