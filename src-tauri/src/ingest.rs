//! Proxy generation and ffmpeg discovery.
//!
//! `model::extract` calls into this module to build thumbnails and locate the
//! `ffmpeg`/`ffprobe` binaries. Thumbnailing and transcoding are Phase 1
//! indexer work, out of scope for this rebuild (shell + Library view), so the
//! proxy makers here are honest no-ops: they report "not made" rather than
//! fabricating a thumbnail. `proxy.state` already has a `failed` value for
//! exactly this case, so no view needs to change once real generation lands.
//!
//! `exif_orientation` is implemented for real, since it is a small, pure read
//! of a tag already needed by `model::extract`.

use std::path::{Path, PathBuf};

pub fn image_proxy_for(_path: &Path, _hash: &str, _proxies_dir: &Path) -> Option<String> {
    None
}

pub fn make_video_proxy(_path: &Path, _hash: &str, _proxies_dir: &Path) -> Option<PathBuf> {
    None
}

pub fn make_audio_proxy(_path: &Path, _hash: &str, _proxies_dir: &Path) -> Option<PathBuf> {
    None
}

/// EXIF orientation tag (1–8), or 1 (no rotation) when absent or unreadable.
pub fn exif_orientation(path: &Path) -> u16 {
    let Ok(file) = std::fs::File::open(path) else {
        return 1;
    };
    let mut buf = std::io::BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut buf) else {
        return 1;
    };
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .map(|v| v as u16)
        .unwrap_or(1)
}

/// Search `PATH` for `ffmpeg`. No bundled binary, no hardcoded install
/// location — if it's not on `PATH`, av attribute extraction is skipped.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(exe))
            .find(|p| p.is_file())
    })
}
