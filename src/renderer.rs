//! Renderer selection with crash-marker based fallback.
//!
//! The original launcher decides between OpenGL ES and native Vulkan at launch
//! time. A marker file records an in-progress Vulkan boot; if the process dies
//! before the renderer confirms itself, the next launch falls back to GLES
//! instead of crash-looping.
//!
//! This module reproduces that protocol without any Android dependency, so it
//! can be unit-tested on the host.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The token that requests the native Vulkan renderer.
pub const VULKAN_TOKEN: &str = "-sourcevr_native_vulkan";

/// Minimum `android.hardware.vulkan.version` value that supports the port.
///
/// `4198400` == `0x401000` == Vulkan 1.1.
pub const VULKAN_1_1: i32 = 4_198_400;

/// Marker written before a Vulkan boot starts.
pub const MARKER_PENDING: &str = "pending";

/// Reason recorded when the process died before the first Vulkan frame.
pub const REASON_INCOMPLETE: &str = "previous-boot-incomplete";

/// Reason recorded when the Vulkan device was lost mid-session.
pub const REASON_DEVICE_LOST: &str = "vulkan-device-lost";

/// Reason recorded when the device reports a Vulkan version below 1.1.
pub const REASON_NO_VULKAN: &str = "device-vulkan-version";

const MARKER_DIR: &str = "renderer-v1";
const MARKER_FILE: &str = "vulkan-fallback";
const MAX_MARKER_BYTES: u64 = 512;

/// Outcome of the launch-time renderer decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Whether the engine should be started with the Vulkan renderer.
    pub vulkan: bool,
    /// Human-readable explanation when a requested Vulkan start was declined.
    pub notice: Option<String>,
}

impl Launch {
    fn gl() -> Self {
        Self {
            vulkan: false,
            notice: None,
        }
    }

    fn vulkan() -> Self {
        Self {
            vulkan: true,
            notice: None,
        }
    }
}

/// Path of the fallback marker inside the app's files directory.
pub fn marker_path(files_dir: &Path) -> PathBuf {
    files_dir.join(MARKER_DIR).join(MARKER_FILE)
}

/// True when `args` contains the Vulkan token (case-insensitive).
pub fn selects_vulkan<'a>(args: impl IntoIterator<Item = &'a str>) -> bool {
    args.into_iter()
        .any(|a| a.trim().eq_ignore_ascii_case(VULKAN_TOKEN))
}

/// True when the reported Vulkan feature level supports the port.
pub fn device_supports_vulkan(reported_version: i32) -> bool {
    reported_version >= VULKAN_1_1
}

/// Reads the marker contents, trimmed.
///
/// Returns `None` when the marker does not exist. Unreadable markers are
/// reported as a synthetic reason rather than silently ignored, matching the
/// original behaviour.
pub fn read_marker(files_dir: &Path) -> Option<String> {
    let path = marker_path(files_dir);
    match fs::read(&path) {
        Ok(bytes) => {
            let slice = &bytes[..bytes.len().min(MAX_MARKER_BYTES as usize)];
            Some(String::from_utf8_lossy(slice).trim().to_owned())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => Some(format!("reason=unreadable-marker detail={}", e.kind())),
    }
}

/// Returns the recorded fallback reason, or `None` when the marker is absent or
/// merely `pending` (which means the previous boot is still in flight).
pub fn read_fallback(files_dir: &Path) -> Option<String> {
    match read_marker(files_dir)? {
        m if m == MARKER_PENDING => None,
        m if m.is_empty() => Some("reason=unknown detail=".to_owned()),
        m => Some(m),
    }
}

/// Atomically writes the marker.
pub fn write_marker(files_dir: &Path, value: &str) -> io::Result<()> {
    let path = marker_path(files_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, value.as_bytes())?;
    fs::rename(&tmp, &path)
}

/// Deletes the marker if it is still `pending` (a Vulkan boot completed).
pub fn retire_pending(files_dir: &Path) {
    if read_marker(files_dir).as_deref() == Some(MARKER_PENDING) {
        let _ = fs::remove_file(marker_path(files_dir));
    }
}

/// Deletes the marker unconditionally (the user asked to retry Vulkan).
pub fn clear_fallback(files_dir: &Path) -> io::Result<()> {
    match fs::remove_file(marker_path(files_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Decides the renderer for this launch.
///
/// Mirrors the original `prepareLaunch` exactly:
///
/// * Vulkan not requested → GLES, no notice, marker untouched.
/// * Vulkan requested but the device reports < 1.1 → GLES, and a
///   `device-vulkan-version` marker is written so the UI can explain it.
/// * Vulkan requested and the marker is `pending` → the previous boot died
///   before its first frame, so GLES is used and `previous-boot-incomplete`
///   is recorded.
/// * Vulkan requested and any other marker exists → GLES with that reason.
/// * Otherwise → Vulkan, and `pending` is written *before* the engine starts.
///
/// Writing the marker is the caller's side effect here, so the decision stays
/// testable; `write_marker` is invoked for the decline paths.
pub fn prepare_launch(files_dir: &Path, requested: bool, device_version: i32) -> Launch {
    if !requested {
        return Launch::gl();
    }

    let marker = read_marker(files_dir);

    let decline = if !device_supports_vulkan(device_version) {
        Some(failure_body(
            REASON_NO_VULKAN,
            Some("this device does not report Vulkan 1.1"),
        ))
    } else if marker.as_deref() == Some(MARKER_PENDING) {
        Some(failure_body(
            REASON_INCOMPLETE,
            Some("the last Vulkan launch closed before its first frame"),
        ))
    } else {
        match marker {
            Some(existing) => {
                // Any recorded failure keeps the game on GLES until the user
                // clears it from the UI.
                let body = if existing.is_empty() {
                    "reason=unknown detail=".to_owned()
                } else {
                    existing
                };
                return Launch {
                    vulkan: false,
                    notice: Some(notice(&body)),
                };
            }
            None => None,
        }
    };

    if let Some(body) = decline {
        let _ = write_marker(files_dir, &body);
        return Launch {
            vulkan: false,
            notice: Some(notice(&body)),
        };
    }

    match write_marker(files_dir, MARKER_PENDING) {
        Ok(()) => Launch::vulkan(),
        Err(e) => Launch {
            vulkan: false,
            notice: Some(format!(
                "Could not prepare the Vulkan renderer ({}), so this launch uses OpenGL ES.",
                e
            )),
        },
    }
}

/// Full user-facing explanation shown when Vulkan is declined.
pub fn notice(marker: &str) -> String {
    format!(
        "{} ({}), so the game is using OpenGL ES. To retry, open Settings & tools → Launch arguments and press Save.",
        headline(marker),
        describe(marker)
    )
}

/// Adds or removes the Vulkan token in an argument list.
///
/// When enabling, the token is inserted *before* a trailing `+map`/`+load`/
/// `+connect` pair so the engine sees the flag before the command it applies to.
pub fn apply(args: &[String], vulkan: bool) -> Vec<String> {
    let mut out: Vec<String> = args
        .iter()
        .filter(|a| !a.trim().eq_ignore_ascii_case(VULKAN_TOKEN))
        .cloned()
        .collect();

    if vulkan {
        let mut insert_at = out.len();
        if out.len() >= 2 {
            let key = out[out.len() - 2].to_ascii_lowercase();
            if matches!(key.as_str(), "+map" | "+load" | "+connect") {
                insert_at = out.len() - 2;
            }
        }
        out.insert(insert_at, VULKAN_TOKEN.to_owned());
    }
    out
}

/// Extracts the `reason=` field from a marker body.
pub fn reason_string(marker: &str) -> String {
    let trimmed = marker.trim();
    if !trimmed.starts_with("reason=") {
        return String::new();
    }
    let rest = &trimmed[7..];
    match rest.find(" detail=") {
        Some(idx) => rest[..idx].trim().to_owned(),
        None => rest.trim().to_owned(),
    }
}

/// Extracts the `detail=` field, falling back to the reason when absent.
pub fn describe(marker: &str) -> String {
    let trimmed = marker.trim();
    if let Some(idx) = trimmed.find(" detail=") {
        let detail = trimmed[idx + 8..].trim();
        if !detail.is_empty() {
            return detail.to_owned();
        }
        return reason_string(&trimmed[..idx]);
    }
    reason_string(trimmed)
}

/// Short headline for the UI banner.
pub fn headline(marker: &str) -> &'static str {
    if reason_string(marker) == REASON_DEVICE_LOST {
        "The Vulkan renderer stopped during play"
    } else {
        "Vulkan did not start"
    }
}

/// Formats a marker body from a reason and detail, stripping control characters.
pub fn failure_body(reason: &str, detail: Option<&str>) -> String {
    let clean: String = detail
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c == '\r' || c == '\n' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    format!("reason={reason} detail={}\n", clean.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opengdroid_renderer_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn token_detection_is_case_insensitive() {
        assert!(selects_vulkan([
            "-game",
            "garrysmod",
            "-sourcevr_native_vulkan"
        ]));
        assert!(selects_vulkan(["-SourceVR_Native_Vulkan"]));
        assert!(!selects_vulkan(["-game", "garrysmod"]));
    }

    #[test]
    fn device_version_threshold() {
        assert!(!device_supports_vulkan(4_198_399));
        assert!(device_supports_vulkan(VULKAN_1_1));
        assert!(device_supports_vulkan(4_198_401));
    }

    #[test]
    fn not_requested_means_gles_without_notice() {
        let dir = temp_dir("gl");
        let launch = prepare_launch(&dir, false, VULKAN_1_1);
        assert_eq!(launch, Launch::gl());
    }

    #[test]
    fn old_device_is_declined_with_reason() {
        let dir = temp_dir("old");
        let launch = prepare_launch(&dir, true, 4_000_000);
        assert!(!launch.vulkan);
        let notice = launch.notice.unwrap();
        assert!(notice.contains("Vulkan did not start"));
        assert!(notice.contains("does not report Vulkan 1.1"));
        // The decline is recorded so the UI can explain it on the next launch.
        assert_eq!(reason_string(&read_marker(&dir).unwrap()), REASON_NO_VULKAN);
    }

    #[test]
    fn pending_marker_blocks_vulkan_after_crash() {
        let dir = temp_dir("pending");
        write_marker(&dir, MARKER_PENDING).unwrap();
        let launch = prepare_launch(&dir, true, VULKAN_1_1);
        assert!(!launch.vulkan, "pending marker must force GLES");
        let notice = launch.notice.unwrap();
        assert!(notice.contains("closed before its first frame"));
        assert_eq!(
            reason_string(&read_marker(&dir).unwrap()),
            REASON_INCOMPLETE
        );
    }

    #[test]
    fn retire_pending_clears_marker() {
        let dir = temp_dir("retire");
        write_marker(&dir, MARKER_PENDING).unwrap();
        retire_pending(&dir);
        assert_eq!(read_marker(&dir), None);
    }

    #[test]
    fn explicit_failure_marker_is_reported() {
        let dir = temp_dir("lost");
        write_marker(
            &dir,
            &failure_body(REASON_DEVICE_LOST, Some("vk device lost")),
        )
        .unwrap();
        let launch = prepare_launch(&dir, true, VULKAN_1_1);
        assert!(!launch.vulkan);
        let notice = launch.notice.unwrap();
        assert!(notice.contains("The Vulkan renderer stopped during play"));
        assert!(notice.contains("vk device lost"));
    }

    #[test]
    fn clear_fallback_allows_retry() {
        let dir = temp_dir("retry");
        write_marker(&dir, &failure_body(REASON_DEVICE_LOST, None)).unwrap();
        assert!(!prepare_launch(&dir, true, VULKAN_1_1).vulkan);
        clear_fallback(&dir).unwrap();
        assert!(prepare_launch(&dir, true, VULKAN_1_1).vulkan);
    }

    #[test]
    fn apply_inserts_before_start_command() {
        let args: Vec<String> = ["-game", "garrysmod", "+map", "gm_construct"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = apply(&args, true);
        assert_eq!(
            out,
            vec![
                "-game",
                "garrysmod",
                "-sourcevr_native_vulkan",
                "+map",
                "gm_construct"
            ]
        );
    }

    #[test]
    fn apply_removes_token_when_disabling() {
        let args: Vec<String> = ["-game", "garrysmod", VULKAN_TOKEN]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(apply(&args, false), vec!["-game", "garrysmod"]);
    }

    #[test]
    fn marker_fields_parse() {
        let body = failure_body(REASON_DEVICE_LOST, Some("line1\nline2\ttab"));
        assert_eq!(reason_string(&body), REASON_DEVICE_LOST);
        assert_eq!(describe(&body), "line1 line2 tab");
        assert_eq!(headline(&body), "The Vulkan renderer stopped during play");
        assert_eq!(headline("reason=other detail=x"), "Vulkan did not start");
    }

    #[test]
    fn describe_falls_back_to_reason_without_detail() {
        assert_eq!(describe("reason=unknown detail="), "unknown");
    }
}
