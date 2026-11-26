//! Storage path resolution.
//!
//! Mirrors the search order the original launcher uses to find game content on
//! external storage, so an existing GDroid install keeps working unchanged.
//!
//! A candidate only counts as a content root when it actually holds a game
//! (`garrysmod/gameinfo.txt`). A directory that merely exists is not enough:
//! otherwise the empty `srceng` stub the launcher creates wins over a real
//! install under `Download/Garry's Mod`.

use std::path::{Path, PathBuf};

/// Directory name of the game content inside a content root.
pub const GAME_DIR: &str = "garrysmod";

/// File that marks a directory as a usable game install.
pub const GAME_MARKER: &str = "gameinfo.txt";

/// Root of the shared (external) content, searched in the same order as the
/// original launcher: `/sdcard/srceng` first, then emulator/`Download` fallbacks.
pub const SHARED_ROOT_CANDIDATES: &[&str] = &[
    "srceng",
    "Download/Garry's Mod",
    "Download/Garrys Mod",
    "Download/garrysmod",
    "Garry's Mod",
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
    "/storage/emulated/0/Download/Garry's Mod",
    "/sdcard/Download/Garry's Mod",
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

/// True when `root` contains a game: `<root>/garrysmod/gameinfo.txt`.
pub fn has_game(root: &Path) -> bool {
    root.join(GAME_DIR).join(GAME_MARKER).is_file()
}

/// Returns the game directory (`<root>/garrysmod`).
pub fn game_dir(root: &Path) -> PathBuf {
    root.join(GAME_DIR)
}

/// Resolves the content root.
///
/// Two passes: first a candidate that actually holds a game, then — if none
/// does — the first candidate that merely exists, then `external_root/srceng`.
/// The last fallback matches the original launcher, which returns its primary
/// path regardless of existence. Checking for a game first matters because the
/// launcher leaves an empty `srceng` behind even when the real install lives
/// under `Download/Garry's Mod`.
pub fn shared_root(external_root: &Path) -> PathBuf {
    let relative: Vec<PathBuf> = SHARED_ROOT_CANDIDATES
        .iter()
        .map(|c| external_root.join(c))
        .collect();
    let absolute: Vec<&Path> = ABSOLUTE_FALLBACKS.iter().map(Path::new).collect();

    if let Some(path) = relative.iter().find(|p| has_game(p)) {
        return path.clone();
    }
    if let Some(path) = absolute.iter().find(|p| has_game(p)) {
        return path.to_path_buf();
    }
    if let Some(path) = relative.iter().find(|p| p.is_dir()) {
        return path.clone();
    }
    if let Some(path) = absolute.iter().find(|p| p.is_dir()) {
        return path.to_path_buf();
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

    fn touch_game(root: &Path) {
        let dir = root.join(GAME_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(GAME_MARKER), "// test").unwrap();
    }

    #[test]
    fn falls_back_to_srceng_when_nothing_exists() {
        let tmp = std::env::temp_dir().join("opengdroid_nonexistent_root_probe");
        assert_eq!(shared_root(&tmp), tmp.join("srceng"));
    }

    #[test]
    fn prefers_root_that_holds_a_game() {
        let tmp = std::env::temp_dir().join("opengdroid_prefers_game_probe");
        let _ = std::fs::remove_dir_all(&tmp);
        // An empty srceng stub, like the one the launcher leaves behind.
        std::fs::create_dir_all(tmp.join("srceng")).unwrap();
        // The real install.
        touch_game(&tmp.join("Download/Garry's Mod"));

        assert_eq!(
            shared_root(&tmp),
            tmp.join("Download/Garry's Mod"),
            "a populated install must beat an empty srceng stub"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn falls_back_to_existing_directory_without_a_game() {
        let tmp = std::env::temp_dir().join("opengdroid_stub_only_probe");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("srceng")).unwrap();

        assert_eq!(shared_root(&tmp), tmp.join("srceng"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn has_game_requires_the_marker_file() {
        let tmp = std::env::temp_dir().join("opengdroid_marker_probe");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(GAME_DIR)).unwrap();
        assert!(
            !has_game(&tmp),
            "an empty garrysmod directory is not a game"
        );

        std::fs::write(tmp.join(GAME_DIR).join(GAME_MARKER), "// test").unwrap();
        assert!(has_game(&tmp));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn game_dir_appends_the_content_directory() {
        assert_eq!(
            game_dir(Path::new("/sdcard/Download/Garry's Mod")),
            PathBuf::from("/sdcard/Download/Garry's Mod/garrysmod")
        );
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
