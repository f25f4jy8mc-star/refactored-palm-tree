//! Gathering the eight signals the ladder needs.
//!
//! This is the only part of reconciliation that touches the disk or the
//! database. The decision itself is pure and lives in `reconcile`.
//!
//! Order matters for cost, not just correctness. The cheap lookups run first,
//! and the hash — the only expensive one — is computed only when the cheap
//! ones have failed to settle it. On an unchanged library that means no file
//! is ever read.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::reconcile::Observation;

/// A file modified within this window may still be being written, so it is
/// left alone until the next pass. Two seconds is comfortably above the
/// timestamp granularity of every filesystem we care about.
const QUIET_WINDOW: Duration = Duration::from_secs(2);

#[cfg(unix)]
fn inode_of(md: &fs::Metadata) -> (Option<i64>, Option<i64>) {
    use std::os::unix::fs::MetadataExt;
    (Some(md.ino() as i64), Some(md.dev() as i64))
}

#[cfg(not(unix))]
fn inode_of(_md: &fs::Metadata) -> (Option<i64>, Option<i64>) {
    // Windows has file ids but reaching them needs a handle open. Until that is
    // written, the ladder simply never matches on inode there and falls through
    // to the hash rules — slower, and correct.
    (None, None)
}

pub struct FileFacts {
    pub inode: Option<i64>,
    pub device: Option<i64>,
    pub size: i64,
    pub mtime: Option<SystemTime>,
    pub readable: bool,
}

pub fn stat(path: &Path) -> FileFacts {
    match fs::metadata(path) {
        Ok(md) => {
            let (inode, device) = inode_of(&md);
            // Openable, not merely present. A file in a folder we cannot enter
            // still stats on some systems.
            let readable = fs::File::open(path).is_ok();
            FileFacts {
                inode,
                device,
                size: md.len() as i64,
                mtime: md.modified().ok(),
                readable,
            }
        }
        Err(_) => FileFacts {
            inode: None,
            device: None,
            size: 0,
            mtime: None,
            readable: false,
        },
    }
}

/// Zero bytes, or written within the quiet window.
pub fn in_flight(f: &FileFacts, now: SystemTime) -> bool {
    if f.size == 0 {
        return true;
    }
    match f.mtime {
        Some(m) => now.duration_since(m).map(|d| d < QUIET_WINDOW).unwrap_or(true),
        None => false,
    }
}

pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Read `archiva-id` out of a note's YAML frontmatter.
///
/// Deliberately minimal: only the leading `---` block, only a flat `key: value`
/// line, and it stops at the closing fence. A note's identity should not depend
/// on a YAML parser's opinion about anchors and multi-line strings.
pub fn frontmatter_id(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let t = line.trim();
        if t == "---" || t == "..." {
            return None;
        }
        if let Some(rest) = t.strip_prefix("archiva-id:") {
            let v = rest.trim().trim_matches(|c| c == '"' || c == '\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn lookup<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    p: P,
) -> Result<Option<(String, Option<String>)>> {
    Ok(conn
        .query_row(sql, p, |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?)
}

/// Build the observation for one file.
///
/// `is_note` decides whether frontmatter is consulted at all — media cannot
/// carry an id, and pretending otherwise would let a stray `archiva-id:` in a
/// text file hijack a node.
pub fn observe(
    conn: &Connection,
    path: &Path,
    is_note: bool,
    now: SystemTime,
) -> Result<Observation> {
    let facts = stat(path);
    let locator = path.to_string_lossy().to_string();

    let mut o = Observation {
        readable: facts.readable,
        flight: in_flight(&facts, now),
        ..Default::default()
    };

    // Rules 1 and 2 fire before any lookup, so do none.
    if o.flight || !o.readable {
        return Ok(o);
    }

    // --- cheap: by path
    if let Some((id, loc)) = lookup(
        conn,
        "SELECT id, locator FROM node WHERE locator = ?1",
        params![locator],
    )? {
        o.path = true;
        o.node_by_path = Some(id);
        let _ = loc;
    }

    // --- cheap: by (device, inode)
    if let (Some(ino), Some(dev)) = (facts.inode, facts.device) {
        if let Some((id, _)) = lookup(
            conn,
            "SELECT id, locator FROM node WHERE device = ?1 AND inode = ?2",
            params![dev, ino],
        )? {
            o.inode = true;
            o.node_by_inode = Some(id);
        }
    }

    // --- cheap: frontmatter, notes only
    if is_note {
        if let Some(declared) = frontmatter_id(path) {
            o.id_present = true;
            o.declared_id = Some(declared.clone());
            if let Some((id, loc)) = lookup(
                conn,
                "SELECT id, locator FROM node WHERE id = ?1",
                params![declared],
            )? {
                o.id_hit = true;
                o.node_by_id = Some(id);
                o.elsewhere = still_at(loc.as_deref(), &locator);
            }
        }
    }

    // --- expensive: the hash.
    //
    // Skipped when the cheap signals already decide the outcome. If the path
    // and inode both match, the ladder needs the hash to tell rule 6 from
    // rule 7 — but mtime answers that without reading the file. Anything else
    // that has already matched by id has been settled by rules 3–5.
    let needs_hash = !(o.id_present && o.id_hit) && !(o.inode && o.path && mtime_unchanged(conn, &o, &facts)?);

    if needs_hash {
        if let Ok(h) = hash_file(path) {
            if let Some((id, loc)) = lookup(
                conn,
                "SELECT id, locator FROM node WHERE content_hash = ?1",
                params![h],
            )? {
                o.hash = true;
                o.node_by_hash = Some(id);
                // Only meaningful when the match is somewhere other than here.
                if !o.elsewhere {
                    o.elsewhere = still_at(loc.as_deref(), &locator);
                }
            }
        }
    } else {
        // Unchanged by mtime, so the contents match by definition. Rule 6.
        o.hash = o.path;
    }

    Ok(o)
}

/// The matched node still lives at a path that exists and is not this one.
/// This single test is the whole difference between rule 11 and rule 12.
fn still_at(recorded: Option<&str>, here: &str) -> bool {
    match recorded {
        Some(p) if p != here => Path::new(p).exists(),
        _ => false,
    }
}

fn mtime_unchanged(conn: &Connection, o: &Observation, facts: &FileFacts) -> Result<bool> {
    let Some(id) = o.node_by_path.as_ref() else {
        return Ok(false);
    };
    let (stored_mtime, stored_size): (Option<String>, Option<i64>) = conn.query_row(
        "SELECT mtime, size_bytes FROM node WHERE id = ?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if stored_size != Some(facts.size) {
        return Ok(false);
    }
    Ok(match (stored_mtime, facts.mtime) {
        (Some(s), Some(m)) => s == fmt_time(m),
        _ => false,
    })
}

pub fn fmt_time(t: SystemTime) -> String {
    // Microseconds since the epoch, zero-padded to a fixed width so that string
    // comparison and numeric comparison agree — SQLite has no timestamp type.
    //
    // Resolution matters here, not just precision. The missing-sweep asks
    // "which nodes have a last_seen_at older than this scan's marker", so if
    // two scans can share a timestamp the sweep silently matches nothing and
    // deleted files are never reported. Seconds collide constantly; microseconds
    // do not, and `scan` additionally forces the marker to strictly increase.
    let micros = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    format!("{micros:016}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, body: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("archiva-test-{name}"));
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn reads_an_id_from_frontmatter() {
        let p = tmp("fm1.md", "---\narchiva-id: abc-123\ntitle: Bergamo\n---\n\nbody\n");
        assert_eq!(frontmatter_id(&p), Some("abc-123".into()));
        fs::remove_file(p).ok();
    }

    #[test]
    fn quoted_ids_are_unwrapped() {
        let p = tmp("fm2.md", "---\narchiva-id: \"abc-123\"\n---\n");
        assert_eq!(frontmatter_id(&p), Some("abc-123".into()));
        fs::remove_file(p).ok();
    }

    /// An `archiva-id` in the body is not frontmatter and must not be read as
    /// one, or any note quoting this document would hijack a node.
    #[test]
    fn only_the_leading_block_counts() {
        let no_fence = tmp("fm3.md", "archiva-id: abc\n");
        assert_eq!(frontmatter_id(&no_fence), None);
        let in_body = tmp("fm4.md", "---\ntitle: x\n---\n\narchiva-id: abc\n");
        assert_eq!(frontmatter_id(&in_body), None);
        fs::remove_file(no_fence).ok();
        fs::remove_file(in_body).ok();
    }

    #[test]
    fn empty_files_are_treated_as_in_flight() {
        let f = FileFacts {
            inode: None,
            device: None,
            size: 0,
            mtime: Some(SystemTime::UNIX_EPOCH),
            readable: true,
        };
        assert!(in_flight(&f, SystemTime::now()));
    }

    #[test]
    fn a_file_written_a_moment_ago_is_in_flight() {
        let now = SystemTime::now();
        let f = FileFacts {
            inode: None,
            device: None,
            size: 100,
            mtime: Some(now),
            readable: true,
        };
        assert!(in_flight(&f, now));

        let settled = FileFacts {
            mtime: Some(now - Duration::from_secs(60)),
            ..f
        };
        assert!(!in_flight(&settled, now));
    }

    #[test]
    fn hashing_is_stable_and_content_dependent() {
        let a = tmp("h1.bin", "hello");
        let b = tmp("h2.bin", "hello");
        let c = tmp("h3.bin", "hello!");
        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
        assert_ne!(hash_file(&a).unwrap(), hash_file(&c).unwrap());
        for p in [a, b, c] {
            fs::remove_file(p).ok();
        }
    }

    #[test]
    fn a_recorded_path_that_no_longer_exists_is_not_elsewhere() {
        assert!(!still_at(Some("/definitely/not/here.jpg"), "/x/a.jpg"));
        assert!(!still_at(Some("/x/a.jpg"), "/x/a.jpg"), "same path is not elsewhere");
        assert!(!still_at(None, "/x/a.jpg"));
    }
}
