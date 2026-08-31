//! What kind of thing a file is.
//!
//! Two jobs, both pure:
//!
//!   * map an extension to a content type — `jpg` becomes `public.jpeg`;
//!   * walk the conformance graph — `public.jpeg` is a kind of `public.image`,
//!     which is a kind of `public.data`, which is a kind of `public.item`.
//!
//! The closure is written onto each node at index time so "is this an image?"
//! is a lookup rather than a walk. Capabilities are then resolved against it
//! at read time, never stored — see §2.4 of the model document.
//!
//! This must stay in step with `capabilities.ts` on the frontend. There is one
//! test at the bottom that names every type both files know about; if you add
//! a type here, add it there and extend that list.

use std::collections::HashSet;

pub const ITEM: &str = "public.item";
pub const DATA: &str = "public.data";
pub const VIRTUAL: &str = "app.archiva.virtual";

/// Declared parents. Conformance is a graph, not a tree — a note on disk is
/// both a markdown file and an Archiva note, and both branches matter.
fn parents(t: &str) -> &'static [&'static str] {
    match t {
        // The root. This arm must exist: without it `public.item` falls to the
        // catch-all below and gains `public.data` as a parent, which inverts
        // the top of the hierarchy and makes every type — including tags,
        // collectors and board cards — conform to "has bytes on disk".
        "public.item" => &[],

        "public.data" | "app.archiva.virtual" => &["public.item"],

        "public.image" => &["public.data"],
        "public.jpeg" | "public.png" | "public.tiff" | "public.heic" | "public.webp"
        | "public.gif" | "com.microsoft.bmp" | "public.svg-image"
        | "com.adobe.photoshop-image" => &["public.image"],

        "public.audiovisual-content" => &["public.data"],
        "public.movie" | "public.audio" => &["public.audiovisual-content"],
        "public.mpeg-4" | "com.apple.quicktime-movie" | "org.matroska.mkv" | "public.avi"
        | "org.webmproject.webm" => &["public.movie"],
        "public.mp3" | "public.aac-audio" | "org.xiph.flac" | "public.aiff-audio"
        | "com.microsoft.waveform-audio" | "org.xiph.ogg" => &["public.audio"],

        "public.composite-content" => &["public.data"],
        "com.adobe.pdf" => &["public.composite-content"],

        "public.3d-content" => &["public.data"],
        "public.wavefront-obj" | "com.autodesk.fbx" | "public.stl" | "org.khronos.gltf" => {
            &["public.3d-content"]
        }

        "public.text" => &["public.data"],
        "net.daringfireball.markdown" => &["public.text"],

        // The two branches meet here: a note is a note whether it is on disk
        // or in the database, and capabilities are granted at this type. It
        // roots at `public.item` rather than at nothing — a note is an item,
        // and a second root that reaches nothing is how the last bug started.
        "app.archiva.note" => &["public.item"],
        "app.archiva.note.file" => &["net.daringfireball.markdown", "app.archiva.note"],
        "app.archiva.note.inline" => &["app.archiva.virtual", "app.archiva.note"],

        "app.archiva.collector" => &["app.archiva.virtual"],
        "app.archiva.collector.folder" | "app.archiva.collector.board" => {
            &["app.archiva.collector"]
        }

        "app.archiva.tag" => &["app.archiva.virtual"],

        _ => &["public.data"], // unknown but on disk: still an item with bytes
    }
}

/// Extension to content type. Lowercase, no leading dot.
///
/// Covers everything Build 17 accepted, so nothing already in a library stops
/// being recognised. `None` means the walker skips the file entirely.
pub fn for_extension(ext: &str) -> Option<&'static str> {
    Some(match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "public.jpeg",
        "png" => "public.png",
        "tif" | "tiff" => "public.tiff",
        "heic" | "heif" => "public.heic",
        "webp" => "public.webp",
        "gif" => "public.gif",
        "bmp" => "com.microsoft.bmp",
        "svg" => "public.svg-image",
        "psd" => "com.adobe.photoshop-image",

        "mp4" | "m4v" => "public.mpeg-4",
        "mov" => "com.apple.quicktime-movie",
        "mkv" => "org.matroska.mkv",
        "avi" => "public.avi",
        "webm" => "org.webmproject.webm",

        "mp3" => "public.mp3",
        "m4a" | "aac" => "public.aac-audio",
        "flac" => "org.xiph.flac",
        "aif" | "aiff" => "public.aiff-audio",
        "wav" => "com.microsoft.waveform-audio",
        "ogg" => "org.xiph.ogg",

        "pdf" => "com.adobe.pdf",

        "obj" => "public.wavefront-obj",
        "fbx" => "com.autodesk.fbx",
        "stl" => "public.stl",
        "gltf" | "glb" => "org.khronos.gltf",

        "md" | "markdown" => "app.archiva.note.file",

        _ => return None,
    })
}

/// The node type a content type belongs to. Four kinds, and which one a thing
/// is follows from what it is rather than being decided separately.
pub fn node_type(content_type: &str) -> &'static str {
    let tree = closure(content_type);
    if tree.iter().any(|t| t == "app.archiva.note") {
        "note"
    } else if tree.iter().any(|t| t == "app.archiva.collector") {
        "collector"
    } else if tree.iter().any(|t| t == "app.archiva.tag") {
        "tag"
    } else {
        "media"
    }
}

/// Transitive closure, leaf first, no duplicates. This is what gets written to
/// `node.content_type_tree`.
pub fn closure(content_type: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    walk(content_type, &mut out, &mut seen);
    out
}

fn walk(t: &str, out: &mut Vec<String>, seen: &mut HashSet<String>) {
    // The graph rejoins — note.file reaches public.item by two routes — so a
    // visited set is required, not an optimisation.
    if !seen.insert(t.to_string()) {
        return;
    }
    out.push(t.to_string());
    for p in parents(t) {
        walk(p, out, seen);
    }
}

pub fn conforms_to(content_type: &str, ancestor: &str) -> bool {
    closure(content_type).iter().any(|t| t == ancestor)
}

/// Which glyph a row draws. Derived from the closure so no view keeps its own
/// map of types to icons.
pub fn icon_kind(content_type: &str) -> &'static str {
    let tree = closure(content_type);
    let has = |t: &str| tree.iter().any(|x| x == t);
    if has("app.archiva.tag") {
        "tag"
    } else if has("app.archiva.collector.board") {
        "board"
    } else if has("app.archiva.collector") {
        "folder"
    } else if has("app.archiva.note") {
        "note"
    } else if has("public.image") {
        "image"
    } else if has("public.movie") {
        "video"
    } else if has("public.audio") {
        "audio"
    } else if has("public.composite-content") {
        "document"
    } else if has("public.3d-content") {
        "model"
    } else {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_conforms_upward() {
        assert_eq!(
            closure("public.jpeg"),
            vec!["public.jpeg", "public.image", "public.data", "public.item"]
        );
    }

    /// The stated test case. PDF is composite content, so it paginates; it is
    /// not audiovisual, so it never plays. No negative rule anywhere.
    #[test]
    fn pdf_is_composite_but_not_audiovisual() {
        assert!(conforms_to("com.adobe.pdf", "public.composite-content"));
        assert!(conforms_to("com.adobe.pdf", "public.data"));
        assert!(!conforms_to("com.adobe.pdf", "public.audiovisual-content"));
        assert!(!conforms_to("com.adobe.pdf", "public.image"));
    }

    /// The graph rejoins here. Both routes must appear, and `public.item` must
    /// appear exactly once.
    #[test]
    fn a_note_on_disk_has_two_parents() {
        let tree = closure("app.archiva.note.file");
        assert!(tree.contains(&"net.daringfireball.markdown".to_string()));
        assert!(tree.contains(&"app.archiva.note".to_string()));
        assert_eq!(tree.iter().filter(|t| *t == "public.item").count(), 1);
    }

    /// A board text card and a note on disk are the same kind of thing, which
    /// is what lets the `is_board_text` flag disappear.
    #[test]
    fn inline_and_file_notes_share_a_type() {
        for t in ["app.archiva.note.file", "app.archiva.note.inline"] {
            assert!(conforms_to(t, "app.archiva.note"));
            assert_eq!(node_type(t), "note");
        }
        assert!(conforms_to("app.archiva.note.file", "public.data"));
        assert!(!conforms_to("app.archiva.note.inline", "public.data"));
    }

    /// `public.item` is the root and conforms to nothing above itself.
    ///
    /// Pinned because the catch-all arm — which sends unknown types to
    /// `public.data` — will happily swallow the root too if its own arm is ever
    /// removed. The symptom is quiet: every tag, collector and board card
    /// starts reporting `reveal` and `full_res`, and the app offers to show you
    /// a file that was never on disk.
    #[test]
    fn public_item_is_the_root() {
        assert_eq!(closure(ITEM), vec![ITEM]);
        assert!(!conforms_to(ITEM, DATA));
        assert!(!conforms_to(ITEM, VIRTUAL));
    }

    /// Virtual things have no bytes, so nothing may grant them a
    /// file capability by conformance.
    #[test]
    fn virtual_types_never_conform_to_data() {
        for t in [
            "app.archiva.tag",
            "app.archiva.collector.folder",
            "app.archiva.collector.board",
            "app.archiva.note.inline",
        ] {
            assert!(!conforms_to(t, DATA), "{t} claims to have bytes on disk");
            assert!(conforms_to(t, VIRTUAL), "{t} is not virtual");
        }
    }

    #[test]
    fn every_extension_build_17_accepted_still_resolves() {
        let old = [
            "jpg", "jpeg", "png", "webp", "gif", "tif", "tiff", "bmp", "mp4", "mov", "mkv",
            "webm", "avi", "mp3", "wav", "flac", "ogg", "m4a", "aiff", "obj", "fbx", "pdf", "md",
        ];
        for e in old {
            assert!(for_extension(e).is_some(), "{e} stopped being recognised");
        }
    }

    #[test]
    fn extension_matching_ignores_case() {
        assert_eq!(for_extension("JPG"), for_extension("jpg"));
        assert_eq!(for_extension("PDF"), Some("com.adobe.pdf"));
        assert_eq!(for_extension("exe"), None);
    }

    /// Every type either has a declared parent or is a root. An unknown type
    /// falls back to `public.data`, which must not create a cycle.
    #[test]
    fn every_closure_terminates_at_item() {
        let all = [
            "public.jpeg", "public.png", "public.tiff", "public.heic", "public.webp",
            "public.gif", "com.microsoft.bmp", "public.svg-image", "com.adobe.photoshop-image",
            "public.mpeg-4", "com.apple.quicktime-movie", "org.matroska.mkv", "public.avi",
            "org.webmproject.webm", "public.mp3", "public.aac-audio", "org.xiph.flac",
            "public.aiff-audio", "com.microsoft.waveform-audio", "org.xiph.ogg",
            "com.adobe.pdf", "public.wavefront-obj", "com.autodesk.fbx", "public.stl",
            "org.khronos.gltf", "app.archiva.note.file", "app.archiva.note.inline",
            "app.archiva.collector.folder", "app.archiva.collector.board", "app.archiva.tag",
            // The two shared ancestors and the root itself: each is reachable
            // as a leaf in principle, and each is a place a second orphan root
            // could hide.
            "app.archiva.note", "app.archiva.collector", "app.archiva.virtual",
            "public.image", "public.movie", "public.audio", "public.text",
            "application/unheard-of",
        ];
        for t in all {
            assert!(
                closure(t).contains(&ITEM.to_string()),
                "{t} does not reach public.item"
            );
        }
    }

    #[test]
    fn node_type_follows_from_content_type() {
        assert_eq!(node_type("public.jpeg"), "media");
        assert_eq!(node_type("com.adobe.pdf"), "media");
        assert_eq!(node_type("app.archiva.note.file"), "note");
        assert_eq!(node_type("app.archiva.collector.board"), "collector");
        assert_eq!(node_type("app.archiva.tag"), "tag");
    }
}
