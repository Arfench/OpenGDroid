//! Storage path resolution.
//!
//! Mirrors the search order the original launcher uses to find game content on
//! external storage, so an existing GDroid install keeps working unchanged.

use std::path::{Path, PathBuf};

/// Root of the shared (external) content, searched in the same order as the
/// original launcher: `/sdcard/srceng` first, then emulator/`Download` fallbacks.
pub const SHARED_ROOT_CANDIDATES: &[&str] = &[
    "srceng",
    "Download/srceng",
    "$MuMu12Shared/srceng",
    "$MuMu12Shared",
    "$MuMuShared/srceng",
    "$MuMuShared",
    "MuMuPlayerShared/srceng",
    "MuMuPlayerShared",
];

/// Absolute fallbacks used when `Environment.getExternalStorageDirectory()`
/// cannot be resolved (shared storage mounted elsewhere).
pub const ABSOLUTE_FALLBACKS: &[&str] = &[
    "/storage/emulated/0/Download/srceng",
    "/sdcard/Download/srceng",
    "/sdcard/$MuMu12Shared/srceng",
    "/sdcard/$MuMu12Shared",
    "/sdcard/$MuMuShared/srceng",
    "/sdcard/$MuMuShared",
    "/mnt/shared/srceng",
    "/mnt/shared",
    "/sdcard/srceng",
];

/// Common-content fallbacks, checked when `<shared_root>/common` is absent.
pub const COMMON_FALLBACKS: &[&str] = &["SourceAndroid/common", "SourceVRPort/common"];

/// Returns the first candidate that exists as a directory under `external_root`.
///
/// Falls back to `external_root/srceng` when nothing matches, matching the
/// original behaviour of returning the primary path regardless of existence.
pub fn shared_root(external_root: &Path) -> PathBuf {
    for candidate in SHARED_ROOT_CANDIDATES {
        let path = external_root.join(candidate);
        if path.is_dir() {
            return path;
        }
    }
    for candidate in ABSOLUTE_FALLBACKS {
        let path = Path::new(candidate);
        if path.is_dir() {
            return path.to_path_buf();
        }
    }
    external_root.join("srceng")
}

/// Returns `<shared_root>/common` or one of the known fallbacks.
pub fn shared_common_root(external_root: &Path) -> PathBuf {
    let shared = shared_root(external_root);
    let common = shared.join("common");
    if common.is_dir() {
        return common;
    }
    for candidate in COMMON_FALLBACKS {
        let path = external_root.join(candidate);
        if path.is_dir() {
            return path;
        }
    }
    shared
}

/// Where imported game content should be placed.
pub fn import_destination(external_root: &Path, install_folder: Option<&str>) -> PathBuf {
    let base = external_root.join("srceng");
    match install_folder {
        Some(folder) => base.join(folder),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_srceng_when_nothing_exists() {
        let tmp = std::env::temp_dir().join("opengdroid_nonexistent_root_probe");
        assert_eq!(shared_root(&tmp), tmp.join("srceng"));
    }

    #[test]
    fn import_destination_appends_folder() {
        let root = Path::new("/sdcard");
        assert_eq!(
            import_destination(root, Some("garrysmod")),
            PathBuf::from("/sdcard/srceng/garrysmod")
        );
        assert_eq!(
            import_destination(root, None),
            PathBuf::from("/sdcard/srceng")
        );
    }
}
