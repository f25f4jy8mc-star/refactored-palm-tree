//! Where an item comes from, and whether it can be reached right now.
//!
//! Checklist I9 and I10, gaps G1 and G2.
//!
//! `scan` decides two of the four availabilities: it marks what it walked past
//! as `present`, and sweeps everything it did not see to `missing`. That is
//! the right answer for a scan — a walk cannot distinguish a deleted file from
//! an unreadable one without stopping to ask, and the ladder deliberately does
//! not stop. The other two states are decided here, afterwards:
//!
//!   * `permission_denied` — the path is still there and the operating system
//!     will not open it. Badging that as missing sends you looking for a file
//!     that never moved.
//!   * `remote_uncached` — a URL nobody has fetched yet. Not broken; not here.
//!     A single missing flag reports it as damage, which is G1 exactly.
//!
//! The distinction is not cosmetic: `capabilities` gates `preview`, `full_res`
//! and `play` on `availability == "present"`, so a remote item correctly
//! offers `fetch` and nothing that would need bytes it does not have.
//!
//! **What this file does not do:** fetch. Checklist M7 — actually retrieving
//! and caching a remote item — is separate work, and until it exists a remote
//! item stays `remote_uncached` forever. That is the honest state for it, and
//! it is visible rather than looking like a broken file.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::io::ErrorKind;
use std::path::Path;

use super::content_type;
use super::scan::uuid_v7;

#[derive(Debug, Serialize)]
pub struct Recheck {
    pub present: usize,
    pub missing: usize,
    pub permission_denied: usize,
    pub remote_uncached: usize,
}

/// What the filesystem says about one path, in the model's four words.
///
/// `NotFound` and `PermissionDenied` are the two the operating system
/// distinguishes and Build 17 threw away. Anything else — a broken symlink
/// chain, an I/O error on a failing disk — is reported as missing, because
/// "the drive is dying" is not one of the four states and pretending the file
/// is readable would be worse.
pub fn classify_path(path: &Path) -> &'static str {
    match std::fs::metadata(path) {
        Ok(_) => "present",
        Err(e) if e.kind() == ErrorKind::PermissionDenied => "permission_denied",
        Err(_) => "missing",
    }
}

/// Re-examine every local item that is not currently `present`, and refine
/// what the scan could only call missing.
///
/// Also promotes back to `present`: a drive that is plugged in again should
/// not need a full walk before its items stop being badged.
pub fn recheck(conn: &Connection) -> Result<Recheck> {
    let mut q = conn.prepare(
        "SELECT id, locator, source_kind, availability FROM node
          WHERE availability <> 'present' AND locator IS NOT NULL",
    )?;
    let rows: Vec<(String, String, String, String)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    drop(q);

    let mut out = Recheck {
        present: 0,
        missing: 0,
        permission_denied: 0,
        remote_uncached: 0,
    };
    for (id, locator, source_kind, was) in rows {
        // A URL is not a path. Asking the filesystem about one would report
        // every remote item as missing, which is the bug this exists to stop.
        let now = if source_kind == "remote_url" {
            "remote_uncached"
        } else {
            classify_path(Path::new(&locator))
        };
        match now {
            "present" => out.present += 1,
            "missing" => out.missing += 1,
            "permission_denied" => out.permission_denied += 1,
            _ => out.remote_uncached += 1,
        }
        if now != was {
            conn.execute(
                "UPDATE node SET availability = ?2, modified_at = datetime('now')
                  WHERE id = ?1",
                params![id, now],
            )?;
        }
    }
    Ok(out)
}

/// Add an item that lives at a URL rather than on disk.
///
/// The row is deliberately thin: `locator` holds the URL and every filesystem
/// column stays NULL, which is what §1.2 made them nullable for. The partial
/// unique index on `locator` only covers `local_file`, so this cannot collide
/// with a path — and two different URLs are two different items, checked here
/// rather than by an index that would also have to cover paths.
pub fn add_remote(conn: &Connection, url: &str, title: Option<&str>) -> Result<String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(anyhow!("not a web address: {url}"));
    }
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM node WHERE source_kind = 'remote_url' AND locator = ?1",
            params![url],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = existing {
        return Ok(id);
    }

    let ct = content_type_for_url(url);
    let name = title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| name_from_url(url));

    let id = uuid_v7();
    conn.execute(
        "INSERT INTO node(id, node_type, content_type, content_type_tree, title,
                          source_kind, locator, availability,
                          display_name, display_subtitle, icon_kind)
         VALUES (?1, ?2, ?3, ?4, ?5, 'remote_url', ?6, 'remote_uncached', ?5, ?7, ?8)",
        params![
            id,
            content_type::node_type(ct),
            ct,
            serde_json::to_string(&content_type::closure(ct))?,
            name,
            url,
            "Not fetched yet",
            content_type::icon_kind(ct),
        ],
    )?;
    Ok(id)
}

/// Guess from the address, and say `public.data` when it does not say.
///
/// A URL with no useful extension is still an item worth keeping; giving it
/// the root data type means it conforms to nothing specific and so is granted
/// only the capabilities everything has — which is the correct answer for a
/// thing whose content nobody has seen.
fn content_type_for_url(url: &str) -> &'static str {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url);
    path.rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .and_then(content_type::for_extension)
        .unwrap_or(content_type::DATA)
}

/// The last meaningful part of the address, without its extension.
///
/// Host and path are separated before anything is stripped. Treating the whole
/// tail as a filename turns `example.com` into `example`, because a hostname's
/// dots look exactly like an extension from the right-hand end.
fn name_from_url(url: &str) -> String {
    let trimmed = url.split(['?', '#']).next().unwrap_or(url);
    let after_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let (host, path) = match after_scheme.split_once('/') {
        Some((h, p)) => (h, p.trim_end_matches('/')),
        None => (after_scheme, ""),
    };
    if path.is_empty() {
        return host.to_string();
    }
    let last = path.rsplit('/').next().unwrap_or(path);
    last.rsplit_once('.')
        .map(|(stem, _)| stem)
        .filter(|s| !s.is_empty())
        .unwrap_or(last)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c.execute_batch(include_str!("../../migrations_model/001_model.sql"))
            .unwrap();
        c
    }

    fn availability(c: &Connection, id: &str) -> String {
        c.query_row(
            "SELECT availability FROM node WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// The repo's convention rather than a dev-dependency: a directory named
    /// after a fresh id, so two tests running at once never share one.
    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("archiva-identity-{}", uuid_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn local(c: &Connection, id: &str, locator: &str, avail: &str) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,locator,availability)
             VALUES (?1,'media','public.jpeg',?1,?2,?3)",
            params![id, locator, avail],
        )
        .unwrap();
    }

    #[test]
    fn a_file_that_is_back_stops_being_badged_without_a_full_walk() {
        let dir = scratch();
        let path = dir.join("a.jpg");
        std::fs::write(&path, b"x").unwrap();
        let c = db();
        local(&c, "a", path.to_str().unwrap(), "missing");
        let r = recheck(&c).unwrap();
        assert_eq!(r.present, 1);
        assert_eq!(availability(&c, "a"), "present");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_is_really_gone_stays_missing() {
        let c = db();
        local(&c, "a", "/definitely/not/here.jpg", "missing");
        recheck(&c).unwrap();
        assert_eq!(availability(&c, "a"), "missing");
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_told_apart_from_a_deleted_one() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        let locked = dir.join("locked");
        std::fs::create_dir(&locked).unwrap();
        let path = locked.join("a.jpg");
        std::fs::write(&path, b"x").unwrap();
        // Take execute off the directory: the file exists and cannot be
        // stat'ed. Running as root defeats this, so the test says so rather
        // than passing for the wrong reason.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let denied = std::fs::metadata(&path)
            .err()
            .is_some_and(|e| e.kind() == ErrorKind::PermissionDenied);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !denied {
            eprintln!("skipped: this process can read anything (running as root?)");
            return;
        }

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let c = db();
        local(&c, "a", path.to_str().unwrap(), "missing");
        let r = recheck(&c).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(r.permission_denied, 1);
        assert_eq!(availability(&c, "a"), "permission_denied");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_url_is_never_asked_of_the_filesystem() {
        let c = db();
        let id = add_remote(&c, "https://example.com/photo.jpg", None).unwrap();
        // Was already remote_uncached; a recheck must not decide it is missing.
        let r = recheck(&c).unwrap();
        assert_eq!(r.remote_uncached, 1);
        assert_eq!(r.missing, 0);
        assert_eq!(availability(&c, &id), "remote_uncached");
    }

    #[test]
    fn a_remote_item_keeps_every_filesystem_column_empty() {
        let c = db();
        let id = add_remote(&c, "https://example.com/photo.jpg", None).unwrap();
        let (dir, file, inode, size): (
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
        ) = c
            .query_row(
                "SELECT parent_dir, filename, inode, size_bytes FROM node WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((dir, file, inode, size), (None, None, None, None));
    }

    #[test]
    fn a_url_and_a_path_can_hold_the_same_string_without_colliding() {
        // The unique index is partial on local_file, and this is the case it
        // was made partial for.
        let c = db();
        local(&c, "a", "https://example.com/photo.jpg", "present");
        assert!(add_remote(&c, "https://example.com/photo.jpg", None).is_ok());
    }

    #[test]
    fn adding_the_same_url_twice_is_one_item() {
        let c = db();
        let a = add_remote(&c, "https://example.com/photo.jpg", None).unwrap();
        let b = add_remote(&c, " https://example.com/photo.jpg ", None).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn the_type_comes_from_the_address_where_the_address_says() {
        let c = db();
        let jpg = add_remote(&c, "https://example.com/a/photo.jpg?v=2", None).unwrap();
        let bare = add_remote(&c, "https://example.com/gallery", None).unwrap();
        let ct = |id: &str| -> String {
            c.query_row(
                "SELECT content_type FROM node WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(ct(&jpg), "public.jpeg");
        assert_eq!(ct(&bare), content_type::DATA);
    }

    #[test]
    fn a_remote_item_is_named_from_its_address_unless_told_otherwise() {
        let c = db();
        let a = add_remote(&c, "https://example.com/a/harbour.jpg", None).unwrap();
        let b = add_remote(&c, "https://example.com/", None).unwrap();
        let d = add_remote(&c, "https://example.com/x.jpg", Some("Harbour wall")).unwrap();
        let name = |id: &str| -> String {
            c.query_row(
                "SELECT display_name FROM node WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(name(&a), "harbour");
        assert_eq!(name(&b), "example.com");
        assert_eq!(name(&d), "Harbour wall");
    }

    #[test]
    fn something_that_is_not_a_web_address_is_refused() {
        let c = db();
        assert!(add_remote(&c, "/Users/me/photo.jpg", None).is_err());
        assert!(add_remote(&c, "ftp://example.com/x", None).is_err());
    }
}
