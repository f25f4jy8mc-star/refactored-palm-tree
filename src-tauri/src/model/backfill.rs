//! Carrying an existing library across.
//!
//! The one job that reads both databases. Everything here reads `legacy.*` and
//! writes only to the model schema; nothing writes back.
//!
//! The interesting part is the links. v1 stores tagging and collector
//! membership identically — both are `direction = 'N'` — so which one a row
//! means can only be recovered by looking at what sits on the other end. That
//! is exactly the ambiguity G7 identified, and the backfill is where it gets
//! resolved once and for all: every North link is split by its target's type
//! into `tag_of`, `contains` or `compass_n`.
//!
//! Idempotent. Every migrated node keeps a `legacy_id` attribute, so a second
//! run finds what the first one made instead of duplicating it.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;

use super::content_type;

#[derive(Debug, Default)]
pub struct BackfillReport {
    pub nodes: usize,
    pub tags: usize,
    pub collectors: usize,
    pub notes: usize,
    pub cards: usize,
    pub edges: usize,
    pub board_positions: usize,
    pub dismissals: usize,
    pub skipped_nodes: usize,
    /// Pairs where v1 held both A→B and B→A in the same lateral direction.
    /// Under the corrected reciprocal rule (G23) West and East are symmetric,
    /// so those two rows now say the same thing. Reported rather than silently
    /// deduplicated — the user asserted both, and only they can say whether
    /// that was deliberate.
    pub redundant_lateral_pairs: Vec<(String, String)>,
}

/// `conn` must be a model connection with the v1 database attached as `legacy`
/// — see `model_db::open_for_backfill`.
pub fn run(conn: &mut Connection) -> Result<BackfillReport> {
    let mut r = BackfillReport::default();
    let mut map: HashMap<i64, String> = HashMap::new();

    let tx = conn.transaction()?;

    // ---- nodes -------------------------------------------------------
    {
        let mut q = tx.prepare(
            "SELECT n.id, n.type, n.title, n.created_at, n.modified_at,
                    m.file_path, m.file_hash, m.size_bytes, m.media_kind,
                    m.proxy_path, m.metadata_json, m.missing,
                    nt.file_path AS note_path
             FROM legacy.node n
             LEFT JOIN legacy.media_detail m ON m.node_id = n.id
             LEFT JOIN legacy.note_detail nt ON nt.node_id = n.id
             ORDER BY n.id",
        )?;
        let rows = q.query_map([], |row| {
            Ok(LegacyNode {
                id: row.get(0)?,
                kind: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
                modified_at: row.get(4)?,
                file_path: row.get(5)?,
                file_hash: row.get(6)?,
                size_bytes: row.get(7)?,
                media_kind: row.get(8)?,
                proxy_path: row.get(9)?,
                metadata_json: row.get(10)?,
                missing: row.get::<_, Option<i64>>(11)?.unwrap_or(0) != 0,
                note_path: row.get(12)?,
            })
        })?;

        for row in rows {
            let l = row?;
            if let Some(existing) = existing_for(&tx, l.id)? {
                map.insert(l.id, existing);
                r.skipped_nodes += 1;
                continue;
            }
            let id = new_id(&tx, l.id)?;
            insert_node(&tx, &id, &l)?;
            map.insert(l.id, id);
            r.nodes += 1;
        }
    }

    // ---- kind detail --------------------------------------------------
    r.tags = copy_tags(&tx, &map)?;
    r.collectors = copy_collectors(&tx, &map)?;
    let (notes, cards) = copy_notes(&tx, &map)?;
    r.notes = notes;
    r.cards = cards;

    // ---- edges --------------------------------------------------------
    let (edges, redundant) = copy_links(&tx, &map)?;
    r.edges = edges;
    r.redundant_lateral_pairs = redundant;
    r.board_positions = copy_board_layout(&tx, &map)?;
    r.dismissals = copy_dismissals(&tx, &map)?;

    tx.commit()?;
    Ok(r)
}

struct LegacyNode {
    id: i64,
    kind: String,
    title: String,
    created_at: String,
    modified_at: String,
    file_path: Option<String>,
    file_hash: Option<String>,
    size_bytes: Option<i64>,
    media_kind: Option<String>,
    proxy_path: Option<String>,
    metadata_json: Option<String>,
    missing: bool,
    note_path: Option<String>,
}

fn existing_for(conn: &Connection, legacy_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT node_id FROM attribute WHERE key = 'legacy_id' AND value = ?1",
            params![legacy_id.to_string()],
            |r| r.get(0),
        )
        .optional()?)
}

fn new_id(conn: &Connection, legacy_id: i64) -> Result<String> {
    let id = super::scan::uuid_v7();
    let _ = conn;
    let _ = legacy_id;
    Ok(id)
}

/// v1 recorded a coarse kind — "image", "video". The precise content type comes
/// from the extension where there is one, so a HEIC stops being merely "image"
/// and starts conforming properly.
fn content_type_for(l: &LegacyNode) -> String {
    let path = l.file_path.as_ref().or(l.note_path.as_ref());
    if let Some(p) = path {
        if let Some(ext) = Path::new(p).extension().and_then(|e| e.to_str()) {
            if let Some(ct) = content_type::for_extension(ext) {
                return ct.to_string();
            }
        }
    }
    match (l.kind.as_str(), l.media_kind.as_deref()) {
        ("tag", _) => "app.archiva.tag".into(),
        ("collector", _) => "app.archiva.collector.folder".into(),
        ("note", _) => "app.archiva.note.file".into(),
        (_, Some("image")) => "public.image".into(),
        (_, Some("video")) => "public.movie".into(),
        (_, Some("audio")) => "public.audio".into(),
        (_, Some("model")) => "public.3d-content".into(),
        _ => "public.data".into(),
    }
}

fn insert_node(conn: &Connection, id: &str, l: &LegacyNode) -> Result<()> {
    let ct = content_type_for(l);
    let locator = l.file_path.clone().or(l.note_path.clone());
    let virtual_node = matches!(l.kind.as_str(), "tag" | "collector");

    let availability = if virtual_node {
        "present"
    } else if l.missing {
        "missing"
    } else {
        "present"
    };

    conn.execute(
        "INSERT INTO node(id, node_type, content_type, content_type_tree, title,
                          display_name, icon_kind, source_kind, locator, parent_dir,
                          filename, extension, size_bytes, content_hash,
                          availability, proxy_thumb_ref, proxy_state,
                          created_at, indexed_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?17, ?18)",
        params![
            id,
            content_type::node_type(&ct),
            ct,
            serde_json::to_string(&content_type::closure(&ct))?,
            l.title,
            content_type::icon_kind(&ct),
            if virtual_node { "app_generated" } else { "local_file" },
            locator,
            locator.as_ref().and_then(|p| Path::new(p).parent().map(|d| d.to_string_lossy().to_string())),
            locator.as_ref().and_then(|p| Path::new(p).file_name().map(|f| f.to_string_lossy().to_string())),
            locator.as_ref().and_then(|p| Path::new(p).extension().map(|e| e.to_string_lossy().to_string())),
            l.size_bytes,
            l.file_hash,
            availability,
            l.proxy_path,
            if l.proxy_path.is_some() { "ready" } else { "not_applicable" },
            l.created_at,
            l.modified_at,
        ],
    )?;

    conn.execute(
        "INSERT INTO attribute(node_id, key, value) VALUES (?1, 'legacy_id', ?2)",
        params![id, l.id.to_string()],
    )?;

    // v1 kept extracted metadata as a JSON blob. Unpack it into rows so it can
    // be sorted and filtered, which a blob never could be.
    if let Some(json) = &l.metadata_json {
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(json) {
            for (k, v) in map {
                let (text, num) = match &v {
                    serde_json::Value::Number(n) => (Some(n.to_string()), n.as_f64()),
                    serde_json::Value::String(s) => (Some(s.clone()), None),
                    serde_json::Value::Bool(b) => (Some(b.to_string()), None),
                    _ => (Some(v.to_string()), None),
                };
                conn.execute(
                    "INSERT OR IGNORE INTO attribute(node_id, key, value, value_num)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, k, text, num],
                )?;
            }
        }
    }
    Ok(())
}

fn copy_tags(conn: &Connection, map: &HashMap<i64, String>) -> Result<usize> {
    let mut n = 0;
    let mut q = conn.prepare("SELECT node_id, facet, tier, sort_order FROM legacy.tag_detail")?;
    let rows: Vec<(i64, String, i64, i64)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (legacy, facet, tier, order) in rows {
        let Some(id) = map.get(&legacy) else { continue };
        // Tier is derived from facet, and the schema checks the pair. v1 rows
        // predate the constraint, so recompute rather than trusting what was
        // stored — a mismatched row would abort the whole transaction.
        let tier = correct_tier(&facet).unwrap_or(tier);
        n += conn.execute(
            "INSERT OR IGNORE INTO tag(node_id, facet, tier, sort_order) VALUES (?1,?2,?3,?4)",
            params![id, facet, tier, order],
        )?;
    }
    Ok(n)
}

fn correct_tier(facet: &str) -> Option<i64> {
    Some(match facet {
        "unclassified" => 0,
        "format" | "era" => 1,
        "environment" | "action" => 2,
        "attribute" | "subject" => 3,
        _ => return None,
    })
}

fn copy_collectors(conn: &Connection, map: &HashMap<i64, String>) -> Result<usize> {
    let mut n = 0;
    let mut q =
        conn.prepare("SELECT node_id, kind, promoted_from_tag_id FROM legacy.collector_detail")?;
    let rows: Vec<(i64, String, Option<i64>)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (legacy, kind, promoted) in rows {
        let Some(id) = map.get(&legacy) else { continue };
        n += conn.execute(
            "INSERT OR IGNORE INTO collector(node_id, collector_kind, promoted_from_tag_id)
             VALUES (?1, ?2, ?3)",
            params![id, kind, promoted.and_then(|p| map.get(&p)).cloned()],
        )?;
        // The content type recorded at insert assumed a folder; boards need
        // correcting so `position` and `expand` resolve properly.
        if kind == "board" {
            let ct = "app.archiva.collector.board";
            conn.execute(
                "UPDATE node SET content_type = ?2, content_type_tree = ?3, icon_kind = ?4
                 WHERE id = ?1",
                params![
                    id,
                    ct,
                    serde_json::to_string(&content_type::closure(ct))?,
                    content_type::icon_kind(ct)
                ],
            )?;
        }
    }
    Ok(n)
}

fn copy_notes(conn: &Connection, map: &HashMap<i64, String>) -> Result<(usize, usize)> {
    let mut files = 0;
    let mut cards = 0;

    let mut q = conn.prepare("SELECT node_id FROM legacy.note_detail")?;
    let ids: Vec<i64> = q.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    for legacy in ids {
        let Some(id) = map.get(&legacy) else { continue };
        files += conn.execute(
            "INSERT OR IGNORE INTO note(node_id, storage, body) VALUES (?1, 'file', '')",
            params![id],
        )?;
    }

    // A board text card becomes a note with inline storage (G4). Same type,
    // same capabilities minus reveal, and the is_board_text flag disappears.
    let mut q = conn.prepare("SELECT node_id, body FROM legacy.board_text")?;
    let rows: Vec<(i64, String)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (legacy, body) in rows {
        let Some(id) = map.get(&legacy) else { continue };
        cards += conn.execute(
            "INSERT OR IGNORE INTO note(node_id, storage, body) VALUES (?1, 'inline', ?2)",
            params![id, body],
        )?;
        let ct = "app.archiva.note.inline";
        conn.execute(
            "UPDATE node SET node_type = 'note', content_type = ?2, content_type_tree = ?3,
                             icon_kind = ?4, source_kind = 'app_generated'
             WHERE id = ?1",
            params![
                id,
                ct,
                serde_json::to_string(&content_type::closure(ct))?,
                content_type::icon_kind(ct)
            ],
        )?;
    }
    Ok((files, cards))
}

/// The important one.
///
/// v1's `direction` conflates three different relationships under 'N'. The
/// target's type is what separates them, and doing that here — once, against
/// the whole library — is what makes `remove from this collector` stop being
/// able to hit a tag.
fn copy_links(
    conn: &Connection,
    map: &HashMap<i64, String>,
) -> Result<(usize, Vec<(String, String)>)> {
    let mut n = 0;
    let mut redundant = Vec::new();

    let mut q = conn.prepare(
        "SELECT l.id, l.from_node, l.to_node, l.direction, l.status, l.label,
                l.scope_collector_id, l.origin, l.created_at, t.type
         FROM legacy.link l
         JOIN legacy.node t ON t.id = l.to_node
         ORDER BY l.from_node, l.direction, l.id",
    )?;
    let rows: Vec<LegacyLink> = q
        .query_map([], |r| {
            Ok(LegacyLink {
                from: r.get(1)?,
                to: r.get(2)?,
                direction: r.get(3)?,
                status: r.get(4)?,
                label: r.get(5)?,
                scope: r.get(6)?,
                origin: r.get(7)?,
                created_at: r.get(8)?,
                target_type: r.get(9)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;

    // v1 had no ordinal, so slots had no order of their own. Assign one from
    // the row order within (source, kind), which is stable and matches what
    // the old interface happened to display.
    let mut ordinals: HashMap<(i64, String), i64> = HashMap::new();
    let mut lateral: Vec<(i64, i64, String)> = Vec::new();

    for l in &rows {
        let (Some(src), Some(tgt)) = (map.get(&l.from), map.get(&l.to)) else {
            continue;
        };
        let kind = kind_for(&l.direction, &l.target_type);
        let key = (l.from, kind.to_string());
        let ord = ordinals.entry(key).or_insert(0);

        n += conn.execute(
            "INSERT OR IGNORE INTO edge(id, source_id, target_id, kind, label, ordinal,
                                        scope_collector_id, status, origin, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                super::scan::uuid_v7(),
                src,
                tgt,
                kind,
                l.label,
                *ord,
                l.scope.and_then(|s| map.get(&s)).cloned(),
                l.status,
                l.origin,
                l.created_at,
            ],
        )?;
        *ord += 1;

        if kind == "compass_w" || kind == "compass_e" {
            lateral.push((l.from, l.to, kind.to_string()));
        }
    }

    // G23: West and East are symmetric, so A→B and B→A in the same lateral
    // direction now say one thing twice. v1's interface presented them as
    // separate, so some libraries will have both.
    for (a, b, kind) in &lateral {
        if lateral.iter().any(|(x, y, k)| x == b && y == a && k == kind) && a < b {
            if let (Some(sa), Some(sb)) = (map.get(a), map.get(b)) {
                redundant.push((sa.clone(), sb.clone()));
            }
        }
    }

    Ok((n, redundant))
}

struct LegacyLink {
    from: i64,
    to: i64,
    direction: String,
    status: String,
    label: Option<String>,
    scope: Option<i64>,
    origin: String,
    created_at: String,
    target_type: String,
}

fn kind_for(direction: &str, target_type: &str) -> &'static str {
    match direction {
        "N" => match target_type {
            "tag" => "tag_of",
            "collector" => "contains",
            _ => "compass_n",
        },
        "S" => "compass_s",
        "E" => "compass_e",
        "W" => "compass_w",
        _ => "compass_n",
    }
}

fn copy_board_layout(conn: &Connection, map: &HashMap<i64, String>) -> Result<usize> {
    let mut n = 0;
    let mut q = conn.prepare("SELECT collector_id, node_id, x, y FROM legacy.board_layout")?;
    let rows: Vec<(i64, i64, f64, f64)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (collector, node, x, y) in rows {
        let (Some(c), Some(nd)) = (map.get(&collector), map.get(&node)) else {
            continue;
        };
        // v1 stored position only. Size and stacking get defaults; z is per
        // board, which is what lets one item sit on several at once (G10).
        let payload = serde_json::json!({"x": x, "y": y, "w": 220.0, "h": 160.0, "z": 0});
        n += conn.execute(
            "INSERT OR IGNORE INTO edge(id, source_id, target_id, kind, scope_collector_id, payload)
             VALUES (?1, ?2, ?3, 'board_position', ?4, ?5)",
            params![super::scan::uuid_v7(), nd, c, c, payload.to_string()],
        )?;
    }
    Ok(n)
}

fn copy_dismissals(conn: &Connection, map: &HashMap<i64, String>) -> Result<usize> {
    let mut n = 0;

    let mut q = conn.prepare("SELECT kind, a_id, b_id FROM legacy.discovery_dismissed")?;
    let rows: Vec<(String, i64, i64)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (kind, a, b) in rows {
        let Some(sa) = map.get(&a) else { continue };
        // v1 used a zero where the second id would go, for suggestions that
        // only concern one node. The key is now explicit (G19).
        let key = if b == 0 {
            format!("{kind}:{sa}")
        } else {
            match map.get(&b) {
                Some(sb) => format!("{kind}:{sa}:{sb}"),
                None => continue,
            }
        };
        n += conn.execute(
            "INSERT OR IGNORE INTO dismissed(dismiss_key, kind) VALUES (?1, ?2)",
            params![key, kind],
        )?;
    }

    let mut q = conn.prepare("SELECT a_id, b_id FROM legacy.dismissed_pair")?;
    let rows: Vec<(i64, i64)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (a, b) in rows {
        let (Some(sa), Some(sb)) = (map.get(&a), map.get(&b)) else {
            continue;
        };
        n += conn.execute(
            "INSERT OR IGNORE INTO dismissed(dismiss_key, kind) VALUES (?1, 'duplicate')",
            params![format!("duplicate:{sa}:{sb}")],
        )?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The G7 split, stated as a test. One direction in v1 becomes three kinds
    /// here, decided entirely by what sits on the other end.
    #[test]
    fn north_splits_three_ways_by_target_type() {
        assert_eq!(kind_for("N", "tag"), "tag_of");
        assert_eq!(kind_for("N", "collector"), "contains");
        assert_eq!(kind_for("N", "media"), "compass_n");
        assert_eq!(kind_for("N", "note"), "compass_n");
    }

    #[test]
    fn lateral_and_vertical_directions_carry_over_unchanged() {
        assert_eq!(kind_for("S", "media"), "compass_s");
        assert_eq!(kind_for("E", "media"), "compass_e");
        assert_eq!(kind_for("W", "note"), "compass_w");
    }

    /// v1 predates the facet/tier constraint, so a stored mismatch would abort
    /// the whole backfill. Recompute instead.
    #[test]
    fn tier_is_recomputed_from_facet() {
        assert_eq!(correct_tier("unclassified"), Some(0));
        assert_eq!(correct_tier("format"), Some(1));
        assert_eq!(correct_tier("environment"), Some(2));
        assert_eq!(correct_tier("subject"), Some(3));
        assert_eq!(correct_tier("nonsense"), None);
    }

    #[test]
    fn content_type_is_sharpened_from_the_extension() {
        let heic = LegacyNode {
            id: 1,
            kind: "media".into(),
            title: "x".into(),
            created_at: String::new(),
            modified_at: String::new(),
            file_path: Some("/x/a.HEIC".into()),
            file_hash: None,
            size_bytes: None,
            media_kind: Some("image".into()),
            proxy_path: None,
            metadata_json: None,
            missing: false,
            note_path: None,
        };
        assert_eq!(content_type_for(&heic), "public.heic");

        let unknown = LegacyNode {
            file_path: None,
            media_kind: Some("video".into()),
            ..heic
        };
        assert_eq!(content_type_for(&unknown), "public.movie");
    }
}
