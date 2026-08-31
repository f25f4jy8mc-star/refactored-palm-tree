//! Proxy generation and ffmpeg discovery.
//!
//! Ported from Build 17's `ingest.rs`, which had already solved the two
//! hard parts: correcting for EXIF orientation (a camera stores a portrait
//! shot as landscape pixels plus a rotation flag; skip this and every
//! portrait renders sideways) and finding ffmpeg when the app was launched
//! from Finder rather than a shell, which does not inherit `PATH`.
//!
//! Image proxies need only the `image` crate — no external binary, so they
//! always work. Video and audio proxies shell out to ffmpeg and simply
//! return `None` when it isn't found; `proxy.state` already has a `failed`
//! value for exactly that case, so no view needs to change once it's
//! installed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Bump whenever proxy generation changes, so a rescan regenerates
/// everything rather than leaving stale rows pointing at the old shape.
pub const PROXY_VERSION: i64 = 1;

/// EXIF orientation tag (1-8), or 1 (no rotation) when absent or unreadable.
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

fn apply_orientation(img: image::DynamicImage, orientation: u16) -> image::DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// A JPEG thumbnail, EXIF-rotation corrected, keyed by content hash so an
/// unchanged file never regenerates and two copies of one photo share a
/// thumbnail. Pure Rust — no ffmpeg, so this path always works.
fn make_image_proxy(src: &Path, hash: &str, proxies_dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(proxies_dir)?;
    let out = proxies_dir.join(format!("{hash}.jpg"));
    if out.exists() {
        return Ok(out);
    }
    let img = apply_orientation(image::open(src)?, exif_orientation(src));
    img.thumbnail(512, 512)
        .to_rgb8()
        .save_with_format(&out, image::ImageFormat::Jpeg)?;
    Ok(out)
}

pub fn image_proxy_for(src: &Path, hash: &str, proxies_dir: &Path) -> Option<String> {
    make_image_proxy(src, hash, proxies_dir)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

fn which_on_path(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(bin))
            .find(|p| p.is_file())
    })
}

/// Locate ffmpeg. A packaged app launched from Finder/Explorer doesn't
/// inherit a shell's `PATH`, so check the usual install locations
/// explicitly rather than relying on `PATH` alone.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

    // 1. Bundled alongside the app binary, if one ships there someday —
    //    walk up a few levels to cover target/debug/<app>, <bundle>/Contents/MacOS, etc.
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..6 {
            let Some(d) = dir.clone() else { break };
            for rel in ["resources/ffmpeg", exe_name] {
                let p = d.join(rel);
                if p.is_file() {
                    return Some(p);
                }
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }

    // 2. Common system install locations, for a shell-less launch.
    if !cfg!(windows) {
        for candidate in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/usr/bin/ffmpeg"] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    // 3. Whatever's on PATH — covers `npm run tauri dev` from a shell.
    which_on_path(exe_name)
}

/// Video proxy: a frame grabbed at 1s. `None` if ffmpeg isn't installed —
/// the library still works, rows just show the icon glyph instead.
pub fn make_video_proxy(src: &Path, hash: &str, proxies_dir: &Path) -> Option<PathBuf> {
    let ffmpeg = find_ffmpeg()?;
    std::fs::create_dir_all(proxies_dir).ok()?;
    let out = proxies_dir.join(format!("{hash}.jpg"));
    if out.exists() {
        return Some(out);
    }
    let status = Command::new(ffmpeg)
        .args(["-ss", "1", "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-vf", "scale='min(512,iw)':-2", "-q:v", "4", "-y"])
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    (status.success() && out.exists()).then_some(out)
}

/// Audio proxy: a waveform image, so audio reads as something in a grid
/// rather than a blank tile.
pub fn make_audio_proxy(src: &Path, hash: &str, proxies_dir: &Path) -> Option<PathBuf> {
    let ffmpeg = find_ffmpeg()?;
    std::fs::create_dir_all(proxies_dir).ok()?;
    let out = proxies_dir.join(format!("{hash}.png"));
    if out.exists() {
        return Some(out);
    }
    let status = Command::new(ffmpeg)
        .arg("-i")
        .arg(src)
        .args(["-filter_complex", "showwavespic=s=512x256:colors=#333333", "-frames:v", "1", "-y"])
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    (status.success() && out.exists()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_jpeg(path: &Path, w: u32, h: u32) {
        let img = image::DynamicImage::new_rgb8(w, h);
        img.save_with_format(path, image::ImageFormat::Jpeg).unwrap();
    }

    #[test]
    fn an_image_proxy_is_generated_and_reused() {
        let dir = std::env::temp_dir().join(format!("archiva-ingest-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("photo.jpg");
        write_test_jpeg(&src, 1000, 600);

        let proxies = dir.join("proxies");
        let out = make_image_proxy(&src, "abc123", &proxies).unwrap();
        assert!(out.exists());
        assert_eq!(out.file_name().unwrap().to_str().unwrap(), "abc123.jpg");

        // A thumbnail, not a copy: it must actually be smaller.
        let (w, h) = image::image_dimensions(&out).unwrap();
        assert!(w <= 512 && h <= 512);

        // Regeneration is skipped when the hash already has a proxy — delete
        // the source and confirm the cached proxy is still returned.
        std::fs::remove_file(&src).unwrap();
        let cached = make_image_proxy(&src, "abc123", &proxies).unwrap();
        assert_eq!(cached, out);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_source_file_fails_rather_than_panicking() {
        let dir = std::env::temp_dir().join(format!("archiva-ingest-missing-{}", std::process::id()));
        let result = make_image_proxy(Path::new("/no/such/file.jpg"), "x", &dir);
        assert!(result.is_err());
    }

    #[test]
    fn orientation_6_rotates_a_landscape_source_to_portrait() {
        let img = image::DynamicImage::new_rgb8(100, 50);
        let rotated = apply_orientation(img, 6);
        assert_eq!((rotated.width(), rotated.height()), (50, 100));
    }

    #[test]
    fn an_unrecognised_orientation_value_is_left_alone() {
        let img = image::DynamicImage::new_rgb8(100, 50);
        let same = apply_orientation(img, 1);
        assert_eq!((same.width(), same.height()), (100, 50));
    }
}
