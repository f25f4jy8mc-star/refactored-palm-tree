//! A note's text, for showing it.
//!
//! A note is stored one of two ways (§1.3): `file` keeps the markdown on disk
//! at the node's locator, `inline` keeps it in the `note` table — that pair is
//! what lets a board's text card and a note on disk be the same kind of thing
//! with no `is_board_text` flag (G4). A reader that knew about only one of
//! them would be a second opinion about what a note is.
//!
//! This reads the file, which `extract`'s principle 4 forbids for
//! *measurement* — nothing here classifies, scores or indexes anything. It
//! hands the bytes to a person who asked to look at them, which is what a
//! preview is.
//!
//! Not the search index, deliberately: that copy is stripped of frontmatter
//! and written when the file was last scanned. A preview should show what is
//! on disk now, including the part `scan` skips.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Enough for any note worth reading in a side panel, and a ceiling on what a
/// stray multi-megabyte file can do to the pane. Bytes, not characters: it is
/// a guard on the read, and a character count cannot bound one.
pub const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteBody {
    pub text: String,
    /// True when the file was longer than `MAX_BYTES`. Shown, because silent
    /// truncation of a document is indistinguishable from a document that
    /// ends there.
    pub truncated: bool,
    /// `file` or `inline` — where this text just came from.
    pub storage: String,
}

/// The text of one note. `None` when the node is not a note at all; an error
/// when it is one whose text cannot be read, because "this note is empty" and
/// "this note could not be opened" are different things to be told.
pub fn body(conn: &Connection, id: &str) -> Result<Option<NoteBody>> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT storage, body FROM note WHERE node_id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((storage, inline)) = row else {
        return Ok(None);
    };

    if storage == "inline" {
        return Ok(Some(NoteBody {
            text: inline,
            truncated: false,
            storage,
        }));
    }

    let locator: Option<String> = conn.query_row(
        "SELECT locator FROM node WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    let locator = locator.ok_or_else(|| anyhow!("this note is stored as a file and has no path"))?;
    let (text, truncated) = read_capped(Path::new(&locator))
        .with_context(|| format!("reading {locator}"))?;
    Ok(Some(NoteBody {
        text,
        truncated,
        storage,
    }))
}

/// At most `MAX_BYTES`, cut on a character boundary so the tail of a
/// multi-byte character never arrives as a replacement glyph.
fn read_capped(path: &Path) -> Result<(String, bool)> {
    let bytes = std::fs::read(path)?;
    let truncated = bytes.len() > MAX_BYTES;
    let slice = if truncated { &bytes[..MAX_BYTES] } else { &bytes[..] };
    let text = match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        // Only the cut can leave a partial character; anything else is a file
        // that is not text, and saying so beats showing mojibake.
        Err(e) if truncated && e.valid_up_to() > 0 => {
            std::str::from_utf8(&slice[..e.valid_up_to()])?.to_string()
        }
        Err(_) => return Err(anyhow!("this file is not text")),
    };
    Ok((text, truncated))
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

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "archiva-notetext-{}-{}",
            std::process::id(),
            crate::model::scan::uuid_v7()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn note(c: &Connection, id: &str, storage: &str, body: &str, locator: Option<&str>) {
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name,locator)
             VALUES (?1,'note','app.archiva.note.file',?1,?2)",
            params![id, locator],
        )
        .unwrap();
        c.execute(
            "INSERT INTO note(node_id,storage,body) VALUES (?1,?2,?3)",
            params![id, storage, body],
        )
        .unwrap();
    }

    #[test]
    fn an_inline_note_comes_from_the_table() {
        let c = db();
        note(&c, "n", "inline", "A card on a board.", None);
        let got = body(&c, "n").unwrap().unwrap();
        assert_eq!(got.text, "A card on a board.");
        assert_eq!(got.storage, "inline");
        assert!(!got.truncated);
    }

    #[test]
    fn a_file_note_comes_from_the_file_rather_than_the_indexed_copy() {
        // The `note.body` column holds what the last scan indexed, stripped of
        // frontmatter. A preview shows what is on disk now.
        let dir = scratch();
        let path = dir.join("thoughts.md");
        std::fs::write(&path, "---\narchiva-id: n\n---\n\nWritten since the scan.\n").unwrap();

        let c = db();
        note(&c, "n", "file", "stale indexed copy", Some(path.to_str().unwrap()));
        let got = body(&c, "n").unwrap().unwrap();
        assert!(got.text.contains("Written since the scan."));
        assert!(got.text.contains("archiva-id"), "frontmatter included — it is part of the file");
        assert_eq!(got.storage, "file");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn something_that_is_not_a_note_has_no_text() {
        let c = db();
        c.execute(
            "INSERT INTO node(id,node_type,content_type,display_name)
             VALUES ('p','media','public.jpeg','p')",
            [],
        )
        .unwrap();
        assert!(body(&c, "p").unwrap().is_none());
    }

    #[test]
    fn a_note_whose_file_is_gone_says_so_rather_than_reading_as_empty() {
        let c = db();
        note(&c, "n", "file", "", Some("/nowhere/at/all.md"));
        let err = body(&c, "n").unwrap_err().to_string();
        assert!(err.contains("reading /nowhere/at/all.md"), "{err}");
    }

    #[test]
    fn a_long_file_is_cut_and_says_it_was() {
        let dir = scratch();
        let path = dir.join("long.md");
        std::fs::write(&path, "x".repeat(MAX_BYTES + 10)).unwrap();

        let c = db();
        note(&c, "n", "file", "", Some(path.to_str().unwrap()));
        let got = body(&c, "n").unwrap().unwrap();
        assert_eq!(got.text.len(), MAX_BYTES);
        assert!(got.truncated);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cut_never_lands_inside_a_character() {
        // The last character straddles the cap, so the naive slice would be
        // invalid UTF-8 and arrive as a replacement glyph.
        let dir = scratch();
        let path = dir.join("wide.md");
        let mut text = "a".repeat(MAX_BYTES - 1);
        text.push('é'); // two bytes, one of them past the cap
        text.push_str("tail");
        std::fs::write(&path, &text).unwrap();

        let c = db();
        note(&c, "n", "file", "", Some(path.to_str().unwrap()));
        let got = body(&c, "n").unwrap().unwrap();
        assert_eq!(got.text.len(), MAX_BYTES - 1);
        assert!(got.truncated);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_is_not_text_is_refused_rather_than_shown_as_mojibake() {
        let dir = scratch();
        let path = dir.join("not-text.md");
        std::fs::write(&path, [0xff, 0xfe, 0x00, 0x01]).unwrap();

        let c = db();
        note(&c, "n", "file", "", Some(path.to_str().unwrap()));
        // `{:#}` is the whole chain, which is what `note_body` sends on —
        // Display alone would show only the outer "reading <path>".
        let err = format!("{:#}", body(&c, "n").unwrap_err());
        assert!(err.contains("not text"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
