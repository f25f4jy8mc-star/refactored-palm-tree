//! Measuring files, and making the small copies views draw.
//!
//! This is the debt §1.4 warned about: **a measurement not taken at index time
//! needs a full re-scan to add later.** So extract generously now, even where
//! nothing reads it yet — dimensions, durations, page counts and above all the
//! date a photograph was taken, which is the axis a date-added gallery will
//! actually want (§9.2).
//!
//! Everything lands in `attribute` as key/value, so adding a measurement never
//! needs a migration. `value_num` is populated wherever a number is meaningful,
//! so sorting and range queries never parse text.
//!
//! Proxy generation reuses Build 17's, which already handles orientation and
//! locates ffmpeg in the places a Finder-launched app can actually see.
//!
//! Nothing here reads *into* a file's content — principle 4. Dimensions, codecs
//! and capture dates are all metadata about the file, not recognition of what
//! is in it.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::content_type;
use super::scan::{Extractor, Proxies};
use crate::ingest;

pub struct RealExtractor {
    pub proxies_dir: PathBuf,
    /// Bumped to force every proxy to regenerate on the next scan. Deleting the
    /// folder without bumping this is what left the old build with rows
    /// pointing at files that no longer existed.
    pub proxy_version: i64,
}

type Attr = (String, Option<String>, Option<f64>);

fn text(k: &str, v: impl Into<String>) -> Attr {
    (k.into(), Some(v.into()), None)
}
fn num(k: &str, v: f64) -> Attr {
    (k.into(), Some(format!("{v}")), Some(v))
}

impl Extractor for RealExtractor {
    fn extract(&self, path: &Path, ct: &str) -> Vec<Attr> {
        let tree = content_type::closure(ct);
        let has = |t: &str| tree.iter().any(|x| x == t);
        let mut out = Vec::new();

        if has("public.image") {
            out.extend(image_attrs(path));
        }
        if has("public.audiovisual-content") {
            out.extend(av_attrs(path));
        }
        if has("com.adobe.pdf") {
            out.extend(pdf_attrs(path));
        }
        if has("app.archiva.note") {
            out.extend(note_attrs(path));
        }
        out
    }

    fn version(&self) -> i64 {
        self.proxy_version
    }

    fn proxies(&self, path: &Path, ct: &str, hash: Option<&str>) -> Proxies {
        let tree = content_type::closure(ct);
        let has = |t: &str| tree.iter().any(|x| x == t);
        let Some(hash) = hash else {
            return Proxies::not_applicable(self.proxy_version);
        };

        // Keyed by content hash, so an unchanged file never regenerates and two
        // copies of the same photo share one thumbnail.
        let thumb = if has("public.image") {
            ingest::image_proxy_for(path, hash, &self.proxies_dir)
        } else if has("public.movie") {
            ingest::make_video_proxy(path, hash, &self.proxies_dir)
                .map(|p| p.to_string_lossy().to_string())
        } else if has("public.audio") {
            ingest::make_audio_proxy(path, hash, &self.proxies_dir)
                .map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };

        let applicable = has("public.image") || has("public.audiovisual-content");
        let made = thumb.is_some();
        Proxies {
            thumb_ref: thumb.clone(),
            preview_ref: thumb,
            playable_ref: None, // transcoding is separate work, deliberately deferred
            version: self.proxy_version,
            // `failed` and `not_applicable` are different answers, and a view
            // draws them differently: an icon versus nothing at all.
            state: match (applicable, made) {
                (false, _) => "not_applicable",
                (true, true) => "ready",
                (true, false) => "failed",
            }
            .to_string(),
        }
    }
}

/// Dimensions without decoding the whole file, plus EXIF.
fn image_attrs(path: &Path) -> Vec<Attr> {
    let mut out = Vec::new();
    if let Ok((w, h)) = image::image_dimensions(path) {
        out.push(num("width", w as f64));
        out.push(num("height", h as f64));
        if h > 0 {
            out.push(num("aspect_ratio", w as f64 / h as f64));
        }
        out.push(text(
            "orientation",
            if w > h {
                "landscape"
            } else if h > w {
                "portrait"
            } else {
                "square"
            },
        ));
    }
    out.push(num("exif_orientation", ingest::exif_orientation(path) as f64));
    out.extend(exif_attrs(path));
    out
}

/// The capture date above all (§9.2). For a photo library this is usually the
/// date the user means, and it is not the file's timestamp — copying a folder
/// rewrites that, and the photograph was still taken when it was taken.
///
/// Values are read from their **typed** representation, not from `Display`.
/// An earlier version rendered every field through `display_value()`, which
/// returns the literal text `unknown` for any value whose type code the crate
/// does not recognise — so a stripped or malformed EXIF block produced the
/// string "unknown" rather than nothing, and it got stored as an unparsed date.
fn exif_attrs(path: &Path) -> Vec<Attr> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = std::io::BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut buf) else {
        return Vec::new();
    };

    let mut out = Vec::new();

    // DateTimeOriginal first, DateTime last: the last is when the file was
    // written, which a re-save changes.
    for tag in [
        exif::Tag::DateTimeOriginal,
        exif::Tag::DateTimeDigitized,
        exif::Tag::DateTime,
    ] {
        let Some(f) = exif.get_field(tag, exif::In::PRIMARY) else {
            continue;
        };
        let exif::Value::Ascii(ref parts) = f.value else {
            // Present but not text. Nothing readable here, and recording the
            // crate's placeholder would be worse than recording nothing.
            continue;
        };
        let Some(bytes) = parts.first() else { continue };

        // The crate parses EXIF's own date format properly; the string path is
        // a fallback for cameras that write something slightly off-spec.
        if let Ok(dt) = exif::DateTime::from_ascii(bytes) {
            let s = format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
            );
            if dt.year > 0 {
                let n = s.replace(['-', ':', ' '], "").parse::<f64>().ok();
                out.push(("captured_at".to_string(), Some(s), n));
                break;
            }
        } else {
            let raw = String::from_utf8_lossy(bytes).trim().to_string();
            if raw.is_empty() {
                continue;
            }
            match exif_date_to_sortable(&raw) {
                Some(sortable) => {
                    let n = sortable.replace(['-', ':', ' '], "").parse::<f64>().ok();
                    out.push(("captured_at".to_string(), Some(sortable), n));
                    break;
                }
                // Kept so a parsing gap shows up in the data as the literal
                // value, instead of looking like a photo that was never dated.
                None => out.push(("captured_at_raw".to_string(), Some(raw), None)),
            }
        }
    }

    for (tag, key) in [
        (exif::Tag::Make, "camera_make"),
        (exif::Tag::Model, "camera_model"),
        (exif::Tag::LensModel, "lens"),
        (exif::Tag::FNumber, "aperture"),
        (exif::Tag::ExposureTime, "shutter"),
        (exif::Tag::PhotographicSensitivity, "iso"),
        (exif::Tag::FocalLength, "focal_length"),
    ] {
        if let Some(v) = readable(&exif, tag) {
            out.push(text(key, v));
        }
    }
    out
}

/// A field's value as text, or nothing. Ascii is read from its bytes; anything
/// else falls back to `Display` — but never the crate's `unknown` placeholder,
/// which means "could not interpret" and is not a value.
fn readable(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
    let f = exif.get_field(tag, exif::In::PRIMARY)?;
    let s = match f.value {
        exif::Value::Ascii(ref parts) => String::from_utf8_lossy(parts.first()?).to_string(),
        _ => f.display_value().to_string(),
    };
    let s = s.trim().trim_matches('"').trim().to_string();
    if s.is_empty() || s == "unknown" {
        return None;
    }
    Some(s)
}

/// EXIF writes `2024:06:11 18:04:22`. Colons in the date make it sort wrong and
/// parse wrong everywhere else, so normalise once, here.
/// Normalise a capture date to `YYYY-MM-DD HH:MM:SS`, which sorts correctly as
/// text — the only form the database can compare.
///
/// Deliberately tolerant about what comes in. EXIF itself writes
/// `2024:06:11 18:04:22`, but the crate may render it with dashes, some cameras
/// use a `T` separator, and the value may arrive quoted. An earlier version
/// accepted only the colon form and returned `None` for everything else, which
/// meant no capture date was ever recorded and nothing said so.
fn exif_date_to_sortable(raw: &str) -> Option<String> {
    let cleaned = raw.trim().trim_matches('"').trim();
    let (date, time) = cleaned
        .split_once(' ')
        .or_else(|| cleaned.split_once('T'))?;

    // Either separator, since both appear in the wild.
    let parts: Vec<&str> = date.split([':', '-']).collect();
    if parts.len() != 3 || parts[0].len() != 4 {
        return None;
    }
    if !parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    // A camera with no clock set writes all zeroes. That is not a date, and
    // storing it would sort those photos to the beginning of time.
    if parts[0] == "0000" {
        return None;
    }
    Some(format!(
        "{}-{:0>2}-{:0>2} {}",
        parts[0],
        parts[1],
        parts[2],
        time.trim()
    ))
}

/// Duration and codecs via ffprobe, which ships beside ffmpeg.
///
/// `duration` is what makes a scrubber possible: without it the transport can
/// play but cannot position itself, and `can('seek')` correctly stays false.
fn av_attrs(path: &Path) -> Vec<Attr> {
    let Some(ffprobe) = find_ffprobe() else {
        return Vec::new();
    };
    let Ok(out) = Command::new(ffprobe)
        .args([
            "-v", "error",
            "-show_entries", "format=duration,bit_rate",
            "-show_entries", "stream=codec_name,codec_type,width,height,r_frame_rate,sample_rate,channels",
            "-of", "default=noprint_wrappers=1",
        ])
        .arg(path)
        .output()
    else {
        return Vec::new();
    };
    let text_out = String::from_utf8_lossy(&out.stdout);

    let mut attrs = Vec::new();
    let mut video_codec = None;
    let mut audio_codec = None;
    let mut last_type = String::new();

    for line in text_out.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if v.is_empty() || v == "N/A" {
            continue;
        }
        match k {
            "duration" => {
                if let Ok(n) = v.parse::<f64>() {
                    attrs.push(num("duration", n));
                }
            }
            "bit_rate" => {
                if let Ok(n) = v.parse::<f64>() {
                    attrs.push(num("bitrate", n));
                }
            }
            "codec_type" => last_type = v.to_string(),
            "codec_name" => {
                if last_type == "video" {
                    video_codec = Some(v.to_string());
                } else if last_type == "audio" {
                    audio_codec = Some(v.to_string());
                }
            }
            "width" | "height" => {
                if let Ok(n) = v.parse::<f64>() {
                    attrs.push(num(k, n));
                }
            }
            "r_frame_rate" => {
                if let Some(f) = parse_ratio(v) {
                    attrs.push(num("frame_rate", f));
                }
            }
            "sample_rate" => {
                if let Ok(n) = v.parse::<f64>() {
                    attrs.push(num("sample_rate", n));
                }
            }
            "channels" => {
                if let Ok(n) = v.parse::<f64>() {
                    attrs.push(num("channels", n));
                }
            }
            _ => {}
        }
    }
    if let Some(c) = video_codec {
        // Whether the webview can play the original without a transcode. It
        // gates `can('play')`, so getting it wrong shows a dead play button.
        attrs.push(text("codec_native", native_video(&c).to_string()));
        attrs.push(text("video_codec", c));
    }
    if let Some(c) = audio_codec {
        attrs.push(text("audio_codec", c));
    }
    attrs
}

fn parse_ratio(v: &str) -> Option<f64> {
    let (a, b) = v.split_once('/')?;
    let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
    if b == 0.0 {
        return None;
    }
    Some(a / b)
}

/// What WebKit will play from a plain `<video>` element.
fn native_video(codec: &str) -> bool {
    matches!(codec, "h264" | "hevc" | "vp8" | "vp9" | "av1")
}

fn find_ffprobe() -> Option<PathBuf> {
    let ffmpeg = ingest::find_ffmpeg()?;
    let probe = ffmpeg.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    probe.exists().then_some(probe)
}

/// Page count by counting page objects. Crude, but it needs to answer one
/// question — does this paginate — and a full parser is a dependency this does
/// not justify.
fn pdf_attrs(path: &Path) -> Vec<Attr> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let needle = b"/Type/Page";
    let mut count = 0usize;
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            // "/Type/Pages" is the tree node, not a page.
            let next = bytes.get(i + needle.len()).copied().unwrap_or(b' ');
            if next != b's' {
                count += 1;
            }
        }
        i += 1;
    }
    if count == 0 {
        return Vec::new();
    }
    vec![num("page_count", count as f64)]
}

fn note_attrs(path: &Path) -> Vec<Attr> {
    let Ok(body) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let words = body.split_whitespace().count();
    let headings = body
        .lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .count();
    vec![
        num("word_count", words as f64),
        num("heading_count", headings as f64),
    ]
}

/// Every EXIF field in a file, as the crate sees it. Purely diagnostic: when a
/// value will not read, this says whether the file has EXIF at all, which tags
/// are present, and what type each value actually is — rather than leaving us
/// to guess from an absence.
pub fn dump_exif(path: &Path) -> Vec<(String, String, String)> {
    let Ok(file) = std::fs::File::open(path) else {
        return vec![(
            "error".into(),
            "cannot open file".into(),
            path.display().to_string(),
        )];
    };
    let mut buf = std::io::BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut buf) {
        Ok(e) => e,
        Err(e) => return vec![("error".into(), "no readable exif".into(), e.to_string())],
    };
    exif.fields()
        .map(|f| {
            let kind = match f.value {
                exif::Value::Ascii(_) => "ascii",
                exif::Value::Short(_) => "short",
                exif::Value::Long(_) => "long",
                exif::Value::Rational(_) => "rational",
                exif::Value::SRational(_) => "srational",
                exif::Value::Byte(_) => "byte",
                exif::Value::Undefined(..) => "undefined",
                _ => "other",
            };
            (
                f.tag.to_string(),
                kind.to_string(),
                f.display_value().to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EXIF colons make a date sort wrong and parse wrong everywhere else.
    /// Every form the crate or a camera might hand us must reach the same
    /// answer. Accepting only the colon form is what silently produced a
    /// library with no capture dates at all.
    #[test]
    fn every_date_form_normalises_to_the_same_value() {
        let want = Some("2024-06-11 18:04:22".to_string());
        for raw in [
            "2024:06:11 18:04:22",
            "2024-06-11 18:04:22",
            "\"2024:06:11 18:04:22\"",
            "2024-06-11T18:04:22",
            "  2024:06:11 18:04:22  ",
            "2024:6:11 18:04:22",
        ] {
            assert_eq!(exif_date_to_sortable(raw), want, "failed on {raw:?}");
        }
    }

    /// The crate prints `unknown` when it cannot interpret a value. That is not
    /// a value, and storing it produced a library full of photos whose capture
    /// date read "unknown" — worse than having none, because it looks like data.
    #[test]
    fn the_crates_placeholder_is_not_treated_as_a_value() {
        assert_eq!(exif_date_to_sortable("unknown"), None);
        assert_eq!(exif_date_to_sortable("\"unknown\""), None);
    }

    #[test]
    fn nonsense_and_unset_clocks_are_refused() {
        assert_eq!(exif_date_to_sortable("not a date"), None);
        assert_eq!(exif_date_to_sortable("24:06:11 18:04:22"), None);
        assert_eq!(exif_date_to_sortable(""), None);
        assert_eq!(exif_date_to_sortable("2024:06:11"), None, "no time part");
        // A camera with no clock set. Storing this sorts those photos to the
        // beginning of time, which is worse than having no date.
        assert_eq!(exif_date_to_sortable("0000:00:00 00:00:00"), None);
    }

    /// Normalised capture dates must sort as strings, since that is how the
    /// database compares them.
    #[test]
    fn capture_dates_sort_chronologically_as_text() {
        let mut ds: Vec<String> = ["2024:12:01 09:00:00", "2024:06:11 18:04:22", "2023:01:05 07:00:00"]
            .iter()
            .filter_map(|d| exif_date_to_sortable(d))
            .collect();
        ds.sort();
        assert_eq!(ds[0], "2023-01-05 07:00:00");
        assert_eq!(ds[2], "2024-12-01 09:00:00");
    }

    #[test]
    fn frame_rates_come_back_as_numbers() {
        assert_eq!(parse_ratio("30000/1001").map(|f| f.round()), Some(30.0));
        assert_eq!(parse_ratio("25/1"), Some(25.0));
        assert_eq!(parse_ratio("0/0"), None);
        assert_eq!(parse_ratio("nonsense"), None);
    }

    /// A codec the webview cannot play must not report as native, or the play
    /// button appears and does nothing.
    #[test]
    fn only_web_playable_codecs_count_as_native() {
        assert!(native_video("h264"));
        assert!(native_video("vp9"));
        assert!(!native_video("prores"));
        assert!(!native_video("mjpeg"));
    }

    #[test]
    fn numeric_attributes_carry_a_sortable_value() {
        // Named `value`, not `text` — a local called `text` shadows the helper
        // of the same name, and the next line's call to it stops compiling.
        let (_, value, n) = num("duration", 90.5);
        assert_eq!(value.as_deref(), Some("90.5"));
        assert_eq!(n, Some(90.5));

        let (_, _, no_number) = text("camera_make", "Fujifilm");
        assert_eq!(no_number, None, "text has nothing to sort numerically");
    }

    #[test]
    fn page_counting_ignores_the_page_tree_node() {
        let dir = std::env::temp_dir().join("archiva-pdf-test.pdf");
        std::fs::write(&dir, b"/Type/Pages /Type/Page x /Type/Page y").unwrap();
        let attrs = pdf_attrs(&dir);
        assert_eq!(attrs[0].2, Some(2.0), "two pages, not three");
        std::fs::remove_file(dir).ok();
    }
}
