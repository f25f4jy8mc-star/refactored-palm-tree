//! What an item can do, right now.
//!
//! Mirrors `capabilities.ts` on the frontend. Two halves, and the split is the
//! whole point (§2.4):
//!
//!   * a capability is **granted** at the highest content type for which it is
//!     always true, and inherited down the conformance graph;
//!   * it is then **gated** by a condition over mutable state — availability,
//!     proxy readiness, item count.
//!
//! So `can()` answers "can, right now", which is the only version a button can
//! act on. Nothing is stored: granting is a set lookup against the materialised
//! closure, gating reads fields the projection already carries, and changing
//! this file takes effect without reindexing anything.

use serde::Serialize;

use super::content_type;

/// The fields a capability decision needs. Every projection that returns rows
/// carries these, so resolution never goes back to the database.
#[derive(Debug, Clone, Default)]
pub struct Subject {
    pub content_type: String,
    pub source_kind: String,
    pub availability: String,
    pub proxy_state: String,
    pub proxy_playable: bool,
    pub writable: bool,
    pub note_storage: Option<String>,
    pub tag_facet: Option<String>,
    pub item_count: i64,
    pub duration: Option<f64>,
    pub page_count: Option<i64>,
    pub codec_native: bool,
}

impl Subject {
    fn present(&self) -> bool {
        self.availability == "present"
    }
}

pub const ALL: &[&str] = &[
    "preview", "full_res", "play", "seek", "queue", "paginate", "orbit", "edit", "embed",
    "expand", "contain", "position", "export", "tag", "link", "rename", "delete", "reveal",
    "fetch", "promote", "set_facet",
];

/// Type-level only: what this kind of thing can ever do.
pub fn granted_by_type(tree: &[String], cap: &str) -> bool {
    let has = |t: &str| tree.iter().any(|x| x == t);
    match cap {
        "preview" | "full_res" | "reveal" | "fetch" => has("public.data"),
        "play" | "seek" | "queue" => has("public.audiovisual-content"),
        "paginate" => has("public.composite-content"),
        "orbit" => has("public.3d-content"),
        "edit" => has("app.archiva.note"),
        "embed" => has("public.image") || has("com.adobe.pdf") || has("app.archiva.note"),
        "expand" | "export" => has("app.archiva.collector"),
        "contain" => has("app.archiva.collector.folder"),
        "position" => has("app.archiva.collector.board"),
        // A tag cannot be tagged: tagging is an edge to a tag, and tag-to-tag
        // relationships are compass links, which `link` already covers.
        "tag" => has("public.item") && !has("app.archiva.tag"),
        "link" | "rename" | "delete" => has("public.item"),
        "promote" => has("app.archiva.tag") || has("app.archiva.note.inline"),
        "set_facet" => has("app.archiva.tag"),
        _ => false,
    }
}

/// Type-level **and** instance-level.
pub fn can(s: &Subject, cap: &str) -> bool {
    let tree = content_type::closure(&s.content_type);
    if !granted_by_type(&tree, cap) {
        return false;
    }
    match cap {
        "preview" => s.proxy_state == "ready" || s.present(),
        "full_res" | "orbit" => s.present(),
        "play" => s.proxy_playable || (s.present() && s.codec_native),
        // Without a duration there is nothing to scrub against, so the
        // transport shows play/pause rather than a scrubber it cannot position.
        "seek" => can(s, "play") && s.duration.is_some(),
        "queue" => can(s, "play"),
        "paginate" => s.page_count.unwrap_or(0) >= 1,
        "edit" => s.note_storage.as_deref() == Some("inline") || (s.present() && s.writable),
        "embed" => can(s, "preview"),
        "export" => s.item_count > 0,
        "reveal" => s.source_kind == "local_file" && s.present(),
        "fetch" => s.source_kind == "remote_url" && s.availability == "remote_uncached",
        "set_facet" => s.tag_facet.as_deref() == Some("unclassified"),
        _ => true,
    }
}

pub fn capabilities_of(s: &Subject) -> Vec<String> {
    ALL.iter()
        .filter(|c| can(s, c))
        .map(|c| c.to_string())
        .collect()
}

/// The single ordered rule that replaces every view's private decision about
/// what a double-click does. Most particular renderer wins.
#[derive(Debug, PartialEq, Serialize)]
pub enum OpenTarget {
    Fetch,
    Expand,
    Edit,
    Play,
    Paginate,
    Orbit,
    Preview,
}

pub fn open_target(s: &Subject) -> Option<OpenTarget> {
    for (cap, target) in [
        ("fetch", OpenTarget::Fetch),
        ("expand", OpenTarget::Expand),
        ("edit", OpenTarget::Edit),
        ("play", OpenTarget::Play),
        ("paginate", OpenTarget::Paginate),
        ("orbit", OpenTarget::Orbit),
        ("preview", OpenTarget::Preview),
    ] {
        if can(s, cap) {
            return Some(target);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(ct: &str) -> Subject {
        Subject {
            content_type: ct.into(),
            source_kind: "local_file".into(),
            availability: "present".into(),
            proxy_state: "ready".into(),
            writable: true,
            ..Default::default()
        }
    }

    /// The stated test case. Paginate yes, play no, and no negative rule
    /// anywhere — PDF simply does not conform to audiovisual content.
    #[test]
    fn pdf_paginates_and_never_plays() {
        let mut s = subject("com.adobe.pdf");
        s.page_count = Some(12);
        assert!(can(&s, "paginate"));
        assert!(!can(&s, "play"));
        assert_eq!(open_target(&s), Some(OpenTarget::Paginate));
    }

    /// The difference between "could" and "can right now".
    #[test]
    fn a_video_without_a_duration_plays_but_does_not_seek() {
        let mut s = subject("public.mpeg-4");
        s.codec_native = true;
        assert!(can(&s, "play"));
        assert!(!can(&s, "seek"));
        s.duration = Some(90.0);
        assert!(can(&s, "seek"));
    }

    #[test]
    fn an_unplugged_drive_removes_capabilities_without_a_reindex() {
        let mut s = subject("public.jpeg");
        assert!(can(&s, "reveal") && can(&s, "full_res"));
        s.availability = "missing".into();
        s.proxy_state = "ready".into();
        assert!(!can(&s, "reveal"), "cannot reveal what is not there");
        assert!(!can(&s, "full_res"));
        assert!(can(&s, "preview"), "the thumbnail still exists");
    }

    #[test]
    fn an_uncached_remote_image_offers_fetch_rather_than_a_broken_preview() {
        let mut s = subject("public.jpeg");
        s.source_kind = "remote_url".into();
        s.availability = "remote_uncached".into();
        s.proxy_state = "pending".into();
        assert!(can(&s, "fetch"));
        assert!(!can(&s, "reveal"));
        assert_eq!(open_target(&s), Some(OpenTarget::Fetch));
    }

    #[test]
    fn an_empty_collector_expands_but_cannot_export() {
        let mut s = subject("app.archiva.collector.folder");
        assert!(can(&s, "expand"));
        assert!(!can(&s, "export"));
        s.item_count = 3;
        assert!(can(&s, "export"));
    }

    /// A board text card is a note with no file. It must not offer to reveal
    /// one.
    ///
    /// `availability` is `present` here on purpose: that is what the backfill
    /// writes for virtual nodes, and an earlier version of this test used
    /// `missing`, which hid a real bug — the conformance root was leaking
    /// `public.data` into every type, so cards were granted `reveal` and
    /// `full_res` and only the availability check was denying them.
    #[test]
    fn a_board_card_is_a_note_minus_reveal() {
        let mut s = subject("app.archiva.note.inline");
        s.note_storage = Some("inline".into());
        s.source_kind = "app_generated".into();
        assert!(can(&s, "edit"));
        assert!(can(&s, "promote"));
        assert!(!can(&s, "reveal"), "there is no file to show");
        assert!(!can(&s, "full_res"));
        assert!(!can(&s, "fetch"));
    }

    #[test]
    fn tags_cannot_be_tagged_but_can_be_linked() {
        let mut s = subject("app.archiva.tag");
        s.tag_facet = Some("unclassified".into());
        assert!(!can(&s, "tag"));
        assert!(can(&s, "link"));
        assert!(can(&s, "set_facet"));
        s.tag_facet = Some("subject".into());
        assert!(!can(&s, "set_facet"), "already classified");
    }

    /// Resolution must be total: every capability answers for every type,
    /// with no panic and no unreachable branch.
    #[test]
    fn resolution_is_total() {
        let types = [
            "public.jpeg", "com.adobe.pdf", "public.mpeg-4", "public.mp3",
            "app.archiva.note.file", "app.archiva.note.inline", "app.archiva.tag",
            "app.archiva.collector.folder", "app.archiva.collector.board",
            "public.wavefront-obj", "application/unheard-of",
        ];
        for t in types {
            let s = subject(t);
            for c in ALL {
                let _ = can(&s, c);
            }
        }
    }
}
