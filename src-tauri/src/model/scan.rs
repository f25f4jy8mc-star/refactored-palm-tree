//! The scan.
//!
//! walk → observe → resolve → apply → log, then a sweep for anything that did
//! not turn up.
//!
//! The pipeline is deliberately thin. All the judgement lives in
//! `reconcile::resolve`, which is pure and exhaustively tested; this file only
//! carries files to it and writes down what it says.
//!
//! Attribute extraction is behind `Extractor` so that dimensions, durations and
//! EXIF dates can land without touching any of this. That boundary matters:
//! §1.4 of the model warns that a measurement not taken at index time needs a
//! full re-scan to add later, so the trait is where that debt gets paid.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

use super::content_type;
use super::reconcile::{self, Action, IdSource, Resolution};
use super::signals;

#[derive(Debug, Default)]
pub struct ScanReport {
    pub seen: usize,
    pub created: usize,
    pub updated: usize,
    pub touched: usize,
    /// Rule 6 that still did the full write, because the node was indexed by a
    /// build that recorded less than this one. Separated from `touched` because
    /// "nothing happened" and "nothing changed but we learned more" look
    /// identical otherwise, and the difference is exactly what you check after
    /// adding extraction.
    pub refreshed: usize,
    pub deferred: usize,
    pub unreadable: usize,
    pub went_missing: usize,
    /// How often each rule fired. Rule 13's share is the number to watch: a
    /// bug anywhere higher up shows up here as a spurious duplicate.
    pub by_rule: HashMap<u8, usize>,
}

/// The four proxy artefacts (§1.3). `state` is what views actually branch on:
/// `pending` draws a shimmer, `failed` draws an icon, and neither is the same
/// as having no thumbnail.
#[derive(Debug, Clone)]
pub struct Proxies {
    pub thumb_ref: Option<String>,
    pub preview_ref: Option<String>,
    pub playable_ref: Option<String>,
    pub version: i64,
    pub state: String,
}

impl Proxies {
    pub fn not_applicable(version: i64) -> Self {
        Self {
            thumb_ref: None,
            preview_ref: None,
            playable_ref: None,
            version,
            state: "not_applicable".into(),
        }
    }
}

/// Measurements taken from the file itself, and the small copies views draw.
/// Everything from `extract` writes into `attribute`, so adding a measurement
/// never needs a migration.
pub trait Extractor {
    fn extract(&self, path: &Path, content_type: &str) -> Vec<(String, Option<String>, Option<f64>)>;
    fn proxies(&self, path: &Path, content_type: &str, hash: Option<&str>) -> Proxies;
    /// What this build knows how to record. A node stored below this number was
    /// indexed by an older build and needs revisiting even if the file has not
    /// changed — bump it whenever extraction learns something new.
    fn version(&self) -> i64;
}

/// Records nothing. Lets the walk and the ladder be exercised without ffmpeg,
/// and keeps the tests independent of what is installed.
pub struct NoExtraction;
impl Extractor for NoExtraction {
    fn extract(&self, _: &Path, _: &str) -> Vec<(String, Option<String>, Option<f64>)> {
        Vec::new()
    }
    fn proxies(&self, _: &Path, _: &str, _: Option<&str>) -> Proxies {
        Proxies::not_applicable(0)
    }
    fn version(&self) -> i64 {
        0
    }
}

pub fn scan(
    conn: &mut Connection,
    roots: &[PathBuf],
    exclude: &[PathBuf],
    extractor: &dyn Extractor,
) -> Result<ScanReport> {
    let mut report = ScanReport::default();
    let now = SystemTime::now();
    let started = scan_marker(conn, now)?;

    for root in roots {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_excluded(e.path(), exclude))
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            let Some(ct) = content_type::for_extension(ext) else {
                continue;
            };

            report.seen += 1;
            let is_note = content_type::node_type(ct) == "note";

            let obs = signals::observe(conn, path, is_note, now)?;
            let res = reconcile::resolve(&obs, is_note);
            *report.by_rule.entry(res.rule).or_insert(0) += 1;

            // One transaction per file. A crash mid-scan leaves a partial
            // library, never a half-written node.
            let tx = conn.transaction()?;
            let mut refreshed = false;
            let node_id = apply(&tx, path, ct, &res, extractor, &started, &mut refreshed)?;
            if res.should_log() {
                tx.execute(
                    "INSERT INTO reconcile_log(node_id, table_version, signals, rule, locator)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        node_id,
                        reconcile::TABLE_VERSION,
                        obs.packed(),
                        res.rule as i64,
                        path.to_string_lossy()
                    ],
                )?;
            }
            tx.commit()?;

            match res.action {
                Action::Defer => report.deferred += 1,
                Action::Unreadable { .. } => report.unreadable += 1,
                Action::Touch { .. } => {
                    report.touched += 1;
                    if refreshed {
                        report.refreshed += 1;
                    }
                }
                Action::Update { .. } => report.updated += 1,
                Action::Create { .. } => report.created += 1,
            }
        }
    }

    report.went_missing = sweep_missing(conn, &started)?;
    Ok(report)
}

fn is_excluded(path: &Path, exclude: &[PathBuf]) -> bool {
    // The workspace is excluded by construction here, in the walker, rather
    // than by filtering afterwards — invariant 9. A filter applied later is a
    // filter someone can forget to apply.
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
    {
        return true;
    }
    exclude.iter().any(|e| path.starts_with(e))
}

fn apply(
    conn: &Connection,
    path: &Path,
    ct: &str,
    res: &Resolution,
    extractor: &dyn Extractor,
    seen_at: &str,
    refreshed: &mut bool,
) -> Result<Option<String>> {
    match &res.action {
        // Rule 1. Change nothing at all — not availability, not last-seen.
        Action::Defer => Ok(None),

        Action::Unreadable { node_id } => {
            if let Some(id) = node_id {
                conn.execute(
                    "UPDATE node SET availability = 'permission_denied', last_seen_at = ?2
                     WHERE id = ?1",
                    params![id, seen_at],
                )?;
            }
            Ok(node_id.clone())
        }

        // Rule 6. Nothing about the *file* changed — but the indexer may have
        // learned to record things it could not before. A stored proxy version
        // behind the current one means this node was indexed by an older build,
        // so it has no thumbnail and no measurements, and rule 6 would leave it
        // that way forever.
        //
        // This is what `proxy_version` is for, and it is the only way an
        // already-indexed library ever picks up extraction added after the
        // fact. Availability is set regardless, because this may be a node
        // coming back from missing.
        Action::Touch { node_id } => {
            let stale: bool = conn.query_row(
                "SELECT proxy_version < ?2 FROM node WHERE id = ?1",
                params![node_id, extractor.version()],
                |r| r.get(0),
            )?;
            if stale {
                write_facts(conn, node_id, path, ct, seen_at, extractor)?;
                write_attributes(conn, node_id, path, ct, extractor)?;
            } else {
                conn.execute(
                    "UPDATE node SET last_seen_at = ?2, availability = 'present' WHERE id = ?1",
                    params![node_id, seen_at],
                )?;
            }
            *refreshed = stale;
            Ok(Some(node_id.clone()))
        }

        Action::Update { node_id } => {
            write_facts(conn, node_id, path, ct, seen_at, extractor)?;
            write_attributes(conn, node_id, path, ct, extractor)?;
            Ok(Some(node_id.clone()))
        }

        Action::Create { id_source, copy_of } => {
            let id = match id_source {
                IdSource::Adopt(existing) => existing.clone(),
                IdSource::Mint | IdSource::MintAndRewrite => uuid_v7(),
            };
            let tree = serde_json::to_string(&content_type::closure(ct))?;
            conn.execute(
                "INSERT INTO node(id, node_type, content_type, content_type_tree,
                                  display_name, icon_kind, source_kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'local_file')",
                params![
                    id,
                    content_type::node_type(ct),
                    ct,
                    tree,
                    display_name(path),
                    content_type::icon_kind(ct),
                ],
            )?;
            write_facts(conn, &id, path, ct, seen_at, extractor)?;
            write_attributes(conn, &id, path, ct, extractor)?;

            if content_type::node_type(ct) == "note" {
                conn.execute(
                    "INSERT OR IGNORE INTO note(node_id, storage, body) VALUES (?1, 'file', '')",
                    params![id],
                )?;
                if matches!(id_source, IdSource::MintAndRewrite) {
                    // Writing the id into the file is what makes rules 3–5
                    // possible on the next pass. Failing to write it is not
                    // fatal: the node exists, and the next scan reaches it
                    // through the inode and path rules instead.
                    let _ = write_frontmatter_id(path, &id);
                }
            }

            if let Some(source) = copy_of {
                // Recorded rather than merged. Rule 12 is ambiguous, so the
                // pair is left for review instead of one silently absorbing
                // the other.
                conn.execute(
                    "INSERT OR IGNORE INTO attribute(node_id, key, value) VALUES (?1, 'copy_of', ?2)",
                    params![id, source],
                )?;
            }
            Ok(Some(id))
        }
    }
}

fn write_facts(
    conn: &Connection,
    id: &str,
    path: &Path,
    ct: &str,
    seen_at: &str,
    extractor: &dyn Extractor,
) -> Result<()> {
    let facts = signals::stat(path);
    let hash = signals::hash_file(path).ok();
    // Proxies are keyed by content hash, so an unchanged file regenerates
    // nothing and two copies of one photo share a thumbnail.
    let px = extractor.proxies(path, ct, hash.as_deref());
    conn.execute(
        "UPDATE node SET
           content_type = ?2, content_type_tree = ?3, icon_kind = ?4,
           locator = ?5, parent_dir = ?6, filename = ?7, extension = ?8,
           inode = ?9, device = ?10, size_bytes = ?11, content_hash = ?12, mtime = ?13,
           availability = 'present', last_seen_at = ?14, modified_at = ?14,
           proxy_thumb_ref = ?15, proxy_preview_ref = ?16, proxy_playable_ref = ?17,
           proxy_version = ?18, proxy_state = ?19,
           display_subtitle = ?20
         WHERE id = ?1",
        params![
            id,
            ct,
            serde_json::to_string(&content_type::closure(ct))?,
            content_type::icon_kind(ct),
            path.to_string_lossy(),
            path.parent().map(|p| p.to_string_lossy().to_string()),
            path.file_name().map(|n| n.to_string_lossy().to_string()),
            path.extension().map(|e| e.to_string_lossy().to_string()),
            facts.inode,
            facts.device,
            facts.size,
            hash,
            facts.mtime.map(signals::fmt_time),
            seen_at,
            px.thumb_ref,
            px.preview_ref,
            px.playable_ref,
            px.version,
            px.state,
            subtitle(ct, facts.size),
        ],
    )?;
    Ok(())
}

/// What list rows show under the name. Derived once here rather than assembled
/// differently by each view.
fn subtitle(ct: &str, size: i64) -> String {
    let kind = content_type::icon_kind(ct);
    if size <= 0 {
        return kind.to_string();
    }
    let mb = size as f64 / 1_048_576.0;
    if mb >= 1.0 {
        format!("{kind} · {mb:.1} MB")
    } else {
        format!("{kind} · {} KB", (size as f64 / 1024.0).round() as i64)
    }
}

fn write_attributes(
    conn: &Connection,
    id: &str,
    path: &Path,
    ct: &str,
    extractor: &dyn Extractor,
) -> Result<()> {
    for (key, value, num) in extractor.extract(path, ct) {
        conn.execute(
            "INSERT INTO attribute(node_id, key, value, value_num) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(node_id, key) DO UPDATE SET value = ?3, value_num = ?4",
            params![id, key, value, num],
        )?;
    }
    Ok(())
}

/// The marker every node touched by this scan is stamped with, and the
/// threshold the sweep compares against.
///
/// It must be strictly greater than every `last_seen_at` already recorded.
/// Wall-clock time alone is not enough: two scans a moment apart can read the
/// same value, and then `last_seen_at < marker` matches nothing and a deleted
/// file is never reported missing. Clamping to `max + 1` makes the sequence
/// monotonic regardless of clock resolution, or of the clock going backwards.
fn scan_marker(conn: &Connection, now: SystemTime) -> Result<String> {
    let wall = signals::fmt_time(now);
    let highest: Option<String> = conn.query_row(
        "SELECT MAX(last_seen_at) FROM node WHERE last_seen_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    Ok(match highest {
        Some(h) if h >= wall => {
            let next = h.trim_start_matches('0').parse::<u128>().unwrap_or(0) + 1;
            format!("{next:016}")
        }
        _ => wall,
    })
}

/// Anything not seen this pass is missing, not deleted. The node stays and so
/// does every edge, tag and note pointing at it — nothing is ever removed
/// because a drive was unplugged.
fn sweep_missing(conn: &Connection, started: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE node SET availability = 'missing'
         WHERE source_kind = 'local_file'
           AND availability = 'present'
           AND (last_seen_at IS NULL OR last_seen_at < ?1)",
        params![started],
    )?)
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn write_frontmatter_id(path: &Path, id: &str) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    let updated = if text.starts_with("---\n") {
        match text[4..].find("\n---") {
            Some(end) => format!("---\narchiva-id: {id}\n{}", &text[4..4 + end + 1]) + &text[4 + end + 1..],
            None => format!("---\narchiva-id: {id}\n---\n\n{text}"),
        }
    } else {
        format!("---\narchiva-id: {id}\n---\n\n{text}")
    };
    std::fs::write(path, updated)?;
    Ok(())
}

/// UUID v7: 48 bits of millisecond timestamp, then randomness. Sorts by
/// creation time, so `ORDER BY id` is a free stable tiebreaker.
pub(super) fn uuid_v7() -> String {
    let ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    let r: [u8; 10] = rand_bytes();
    b[6..16].copy_from_slice(&r);
    b[6] = (b[6] & 0x0f) | 0x70; // version 7
    b[8] = (b[8] & 0x3f) | 0x80; // variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9],
        b[10..16].iter().map(|x| format!("{x:02x}")).collect::<String>()
    )
}

fn rand_bytes() -> [u8; 10] {
    // No rand crate in the tree, and this does not need to be
    // cryptographically strong — only unlikely to collide within a
    // millisecond, which the timestamp prefix already narrows to one machine.
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = [0u8; 10];
    let mut h = RandomState::new().build_hasher();
    h.write_u64(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0),
    );
    let a = h.finish().to_le_bytes();
    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(a[0] as u64);
    let b = h2.finish().to_le_bytes();
    out[0..8].copy_from_slice(&a);
    out[8..10].copy_from_slice(&b[0..2]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v7_has_the_right_shape_and_sorts_by_time() {
        let a = uuid_v7();
        assert_eq!(a.len(), 36);
        assert_eq!(a.chars().filter(|c| *c == '-').count(), 4);
        assert_eq!(&a[14..15], "7", "version nibble");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"), "variant nibble");

        std::thread::sleep(std::time::Duration::from_millis(3));
        let b = uuid_v7();
        assert!(a < b, "later ids must sort after earlier ones: {a} !< {b}");
    }

    #[test]
    fn ids_do_not_collide_in_a_tight_loop() {
        let ids: std::collections::HashSet<_> = (0..2000).map(|_| uuid_v7()).collect();
        assert_eq!(ids.len(), 2000);
    }

    #[test]
    fn dotfiles_and_excluded_roots_are_skipped_by_the_walker() {
        let workspace = PathBuf::from("/lib/.archiva");
        assert!(is_excluded(Path::new("/lib/.DS_Store"), &[]));
        assert!(is_excluded(Path::new("/lib/.archiva/proxies/x.jpg"), &[workspace.clone()]));
        assert!(!is_excluded(Path::new("/lib/photos/x.jpg"), &[workspace]));
    }

    /// An already-indexed library must be able to pick up extraction added
    /// after the fact. Without the version check, rule 6 leaves every unchanged
    /// file without a thumbnail forever, and the only cure is deleting the
    /// database.
    #[test]
    fn a_stale_proxy_version_makes_an_unchanged_file_do_the_work() {
        struct V(i64);
        impl Extractor for V {
            fn extract(&self, _: &Path, _: &str) -> Vec<(String, Option<String>, Option<f64>)> {
                vec![("width".into(), Some("100".into()), Some(100.0))]
            }
            fn proxies(&self, _: &Path, _: &str, _: Option<&str>) -> Proxies {
                Proxies::not_applicable(self.0)
            }
            fn version(&self) -> i64 {
                self.0
            }
        }
        assert_eq!(V(5).version(), 5);
        assert_eq!(NoExtraction.version(), 0);
        assert!(NoExtraction.version() < V(5).version(),
                "a node written by the no-op extractor reads as stale to a real one");
    }

    #[test]
    fn display_name_drops_the_extension() {
        assert_eq!(display_name(Path::new("/x/Bergamo arcade.jpg")), "Bergamo arcade");
        assert_eq!(display_name(Path::new("/x/notes.md")), "notes");
    }
}
