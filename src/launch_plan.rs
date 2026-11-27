//! Full launch contract: environment, readiness and the resulting plan.
//!
//! The original launcher hands the engine a fixed set of environment
//! variables and a prepared `argv`, then lets SDL call `LauncherMainAndroid`.
//! Everything it sets is reproduced here so a ported front-end can start the
//! engine without guessing, and so a missing prerequisite is reported instead
//! of failing inside the engine with no explanation.

use std::path::{Path, PathBuf};

use crate::launch::{self, Extras, Mode, Renderer};
use crate::storage;

/// Libraries SDL loads before `LauncherMainAndroid` is called.
///
/// Without these the process cannot start at all, so they gate readiness.
pub const ENTRY_LIBRARIES: &[&str] = &["libSDL2.so", "liblauncher.so"];

/// Libraries the engine pulls in after the entry point runs.
///
/// Reported for diagnostics only: a missing one is a broken install, but it
/// does not stop the process from starting.
pub const ENGINE_LIBRARIES: &[&str] = &[
    "libengine.so",
    "libclient.so",
    "libserver.so",
    "libmaterialsystem.so",
    "libstudiorender.so",
    "libvgui2.so",
    "libvguimatsurface.so",
    "libvphysics.so",
    "libfilesystem_stdio.so",
    "libinputsystem.so",
    "libdatacache.so",
    "libtier0.so",
    "libvstdlib.so",
    "libtogl.so",
    "libshaderapidx9.so",
];

/// Environment variable name set by the original launcher.
pub const ENV_APP_DATA_PATH: &str = "APP_DATA_PATH";
pub const ENV_VALVE_GAME_PATH: &str = "VALVE_GAME_PATH";
pub const ENV_APP_LIB_PATH: &str = "APP_LIB_PATH";
pub const ENV_APP_MOD_LIB: &str = "APP_MOD_LIB";
pub const ENV_SOURCEVR_GAME: &str = "SOURCEVR_GAME";
pub const ENV_SOURCE_GAME: &str = "SOURCE_GAME";
pub const ENV_SHARED_CONTENT_PATH: &str = "SOURCEVR_SHARED_CONTENT_PATH";
pub const ENV_WRITE_GAME_PATH: &str = "SOURCEVR_WRITE_GAME_PATH";
pub const ENV_EXTRAS_VPK_PATH: &str = "EXTRAS_VPK_PATH";
pub const ENV_LANG: &str = "LANG";
pub const ENV_TRAP_BACK_BUTTON: &str = "SDL_ANDROID_TRAP_BACK_BUTTON";
pub const ENV_VULKAN_CACHE: &str = "GMOD_FLAT_VULKAN_CACHE";
pub const ENV_TEXTURE_CACHE: &str = "GMOD_FLAT_TEXTURE_CACHE";
pub const ENV_TMPDIR: &str = "TMPDIR";
pub const ENV_SQLITE_TMPDIR: &str = "SQLITE_TMPDIR";

/// Everything the front-end must supply to build a launch plan.
#[derive(Debug, Clone)]
pub struct Request {
    /// Resolved content root (`.../Download/Garry's Mod`).
    pub shared_root: PathBuf,
    /// Game folder id the engine receives, e.g. `garrysmod`.
    pub game_id: String,
    /// Candidate directories for the native libraries, best first.
    ///
    /// The app's own `nativeLibraryDir` comes first; an imported engine
    /// directory can follow it.
    pub library_candidates: Vec<PathBuf>,
    /// Directory the engine may write user data to.
    pub user_dir: PathBuf,
    /// Shared engine content (materials, shaders).
    pub engine_content: PathBuf,
    /// Runtime VPK shipped with the app.
    pub runtime_vpk: PathBuf,
    /// Cache directory for shader and texture caches.
    pub cache_dir: PathBuf,
    pub mode: Mode,
    pub heap_mb: u32,
    pub renderer: Renderer,
    pub extras: Extras,
}

/// What still has to be true before the engine can be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    /// `<shared_root>/garrysmod/gameinfo.txt` exists.
    pub game_installed: bool,
    /// Entry libraries that are absent from `native_lib_dir`.
    pub missing_entry_libraries: Vec<String>,
    /// Engine libraries that are absent from `native_lib_dir`.
    pub missing_engine_libraries: Vec<String>,
}

impl Readiness {
    /// True when the engine could be started.
    pub fn ready(&self) -> bool {
        self.game_installed && self.missing_entry_libraries.is_empty()
    }

    /// Human-readable reasons why launching would fail, most severe first.
    pub fn blockers(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.game_installed {
            out.push("game content is missing (no garrysmod/gameinfo.txt)".to_owned());
        }
        for lib in &self.missing_entry_libraries {
            out.push(format!("entry library is missing: {lib}"));
        }
        out
    }
}

/// A resolved launch: the command line, the environment and the readiness state.
#[derive(Debug, Clone)]
pub struct Plan {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub readiness: Readiness,
    /// Directory the entry libraries were found in (or the preferred candidate).
    pub library_dir: PathBuf,
}

impl Plan {
    /// True when the plan can actually be executed.
    pub fn ready(&self) -> bool {
        self.readiness.ready()
    }

    /// The environment as `KEY=VALUE` pairs, as passed to the child process.
    pub fn env_pairs(&self) -> Vec<String> {
        self.env.iter().map(|(k, v)| format!("{k}={v}")).collect()
    }

    /// The command line as a single string.
    pub fn command_line(&self) -> String {
        self.args.join(" ")
    }
}

/// Separator between candidate library directories in one argument.
///
/// Android paths never contain a colon, so a single string can carry several
/// candidates: the app's own `nativeLibraryDir` first, then an imported engine
/// directory.
pub const LIBRARY_PATH_SEPARATOR: char = ':';

/// Splits `dir1:dir2` into candidate paths, skipping empty entries.
pub fn library_candidates(raw: &str) -> Vec<PathBuf> {
    raw.split(LIBRARY_PATH_SEPARATOR)
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Picks the first candidate that holds every entry library.
///
/// Falls back to the first candidate when none qualifies, so the readiness
/// report keeps pointing at the directory the caller would have used.
pub fn pick_library_dir(candidates: &[PathBuf]) -> PathBuf {
    candidates
        .iter()
        .find(|dir| missing_libraries(dir, ENTRY_LIBRARIES).is_empty())
        .cloned()
        .or_else(|| candidates.first().cloned())
        .unwrap_or_default()
}

/// Checks what is present on disk for `request`.
pub fn check_readiness(request: &Request) -> Readiness {
    let game_installed = request
        .shared_root
        .join(storage::GAME_DIR)
        .join(storage::GAME_MARKER)
        .is_file();
    let lib_dir = pick_library_dir(&request.library_candidates);

    Readiness {
        game_installed,
        missing_entry_libraries: missing_libraries(&lib_dir, ENTRY_LIBRARIES),
        missing_engine_libraries: missing_libraries(&lib_dir, ENGINE_LIBRARIES),
    }
}

fn missing_libraries(dir: &Path, required: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|name| !dir.join(name).is_file())
        .map(|name| (*name).to_owned())
        .collect()
}

/// Builds the environment the engine expects, mirroring the original launcher.
pub fn environment(request: &Request) -> Vec<(String, String)> {
    let write_root = request
        .user_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| request.user_dir.clone());
    let lib_dir = pick_library_dir(&request.library_candidates);

    let pairs = [
        (ENV_APP_DATA_PATH, write_root.to_string_lossy().into_owned()),
        (
            ENV_VALVE_GAME_PATH,
            write_root.to_string_lossy().into_owned(),
        ),
        (ENV_APP_LIB_PATH, lib_dir.to_string_lossy().into_owned()),
        (ENV_APP_MOD_LIB, lib_dir.to_string_lossy().into_owned()),
        (ENV_SOURCEVR_GAME, request.game_id.clone()),
        (ENV_SOURCE_GAME, request.game_id.clone()),
        (
            ENV_SHARED_CONTENT_PATH,
            request.engine_content.to_string_lossy().into_owned(),
        ),
        (
            ENV_WRITE_GAME_PATH,
            request.user_dir.to_string_lossy().into_owned(),
        ),
        (
            ENV_EXTRAS_VPK_PATH,
            request.runtime_vpk.to_string_lossy().into_owned(),
        ),
        (ENV_LANG, "en_US.UTF-8".to_owned()),
        (ENV_TRAP_BACK_BUTTON, "1".to_owned()),
        (
            ENV_VULKAN_CACHE,
            request.cache_dir.to_string_lossy().into_owned(),
        ),
        (
            ENV_TEXTURE_CACHE,
            request.cache_dir.to_string_lossy().into_owned(),
        ),
        (ENV_TMPDIR, request.cache_dir.to_string_lossy().into_owned()),
        (
            ENV_SQLITE_TMPDIR,
            request.cache_dir.to_string_lossy().into_owned(),
        ),
    ];

    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// Builds the full launch plan for `request`.
pub fn plan(request: &Request) -> Plan {
    let game_dir = request.shared_root.join(storage::GAME_DIR);
    let args = launch::build(
        &game_dir.to_string_lossy(),
        request.mode,
        request.heap_mb,
        request.renderer,
        &request.extras,
    );

    Plan {
        args,
        env: environment(request),
        readiness: check_readiness(request),
        library_dir: pick_library_dir(&request.library_candidates),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(root: &Path) -> Request {
        Request {
            shared_root: root.to_path_buf(),
            game_id: "garrysmod".to_owned(),
            library_candidates: vec![root.join("lib")],
            user_dir: root.join("user/garrysmod"),
            engine_content: root.join("common"),
            runtime_vpk: root.join("runtime.zip"),
            cache_dir: root.join("cache"),
            mode: Mode::new(1280, 720, true),
            heap_mb: 1024,
            renderer: Renderer::OpenGlEs,
            extras: Extras::parse("+map gm_construct"),
        }
    }

    /// Paths on Windows use `\`; the assertions are written for Android (`/`).
    fn slashed(value: &str) -> String {
        value.replace('\\', "/")
    }

    #[test]
    fn environment_matches_the_original_contract() {
        let root = Path::new("/sdcard/Download/Garry's Mod");
        let env = environment(&request(root));
        let get = |key: &str| {
            slashed(
                &env.iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
                    .unwrap(),
            )
        };

        assert_eq!(get(ENV_SOURCEVR_GAME), "garrysmod");
        assert_eq!(get(ENV_SOURCE_GAME), "garrysmod");
        assert_eq!(get(ENV_LANG), "en_US.UTF-8");
        assert_eq!(get(ENV_TRAP_BACK_BUTTON), "1");
        assert_eq!(
            get(ENV_WRITE_GAME_PATH),
            "/sdcard/Download/Garry's Mod/user/garrysmod"
        );
        assert_eq!(get(ENV_APP_DATA_PATH), "/sdcard/Download/Garry's Mod/user");
        assert_eq!(
            get(ENV_SHARED_CONTENT_PATH),
            "/sdcard/Download/Garry's Mod/common"
        );
        assert_eq!(get(ENV_TMPDIR), "/sdcard/Download/Garry's Mod/cache");
        assert_eq!(env.len(), 15, "the original sets fifteen variables");
    }

    #[test]
    fn missing_game_blocks_the_launch() {
        let tmp = std::env::temp_dir().join("opengdroid_readiness_game_probe");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let readiness = check_readiness(&request(&tmp));
        assert!(!readiness.game_installed);
        assert!(!readiness.ready());
        assert!(readiness
            .blockers()
            .iter()
            .any(|b| b.contains("gameinfo.txt")));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_entry_libraries_block_the_launch() {
        let tmp = std::env::temp_dir().join("opengdroid_readiness_lib_probe");
        let _ = std::fs::remove_dir_all(&tmp);
        let game = tmp.join("garrysmod");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("gameinfo.txt"), "// test").unwrap();
        std::fs::create_dir_all(tmp.join("lib")).unwrap();

        let readiness = check_readiness(&request(&tmp));
        assert!(readiness.game_installed);
        assert!(!readiness.ready(), "no entry libraries are present");
        assert_eq!(
            readiness.missing_entry_libraries.len(),
            ENTRY_LIBRARIES.len()
        );

        // Providing the entry libraries is enough to become ready.
        for lib in ENTRY_LIBRARIES {
            std::fs::write(tmp.join("lib").join(lib), b"stub").unwrap();
        }
        let readiness = check_readiness(&request(&tmp));
        assert!(readiness.ready());
        assert!(readiness.missing_engine_libraries.len() == ENGINE_LIBRARIES.len());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn plan_carries_args_and_env_together() {
        let root = Path::new("/sdcard/Download/Garry's Mod");
        let plan = plan(&request(root));
        let command = slashed(&plan.command_line());
        assert!(command.contains("-game /sdcard/Download/Garry's Mod/garrysmod"));
        assert!(command.contains("-heapsize 1024"));
        assert!(command.ends_with("+map gm_construct"));
        assert_eq!(plan.env_pairs().len(), 15);
        assert!(plan
            .env_pairs()
            .contains(&"SOURCEVR_GAME=garrysmod".to_owned()));
    }

    #[test]
    fn library_candidates_splits_android_style_paths() {
        let parts = library_candidates("/data/app/lib:/sdcard/engine/lib");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], PathBuf::from("/data/app/lib"));
        assert_eq!(parts[1], PathBuf::from("/sdcard/engine/lib"));
        assert!(library_candidates("").is_empty());
        assert_eq!(library_candidates("/only/one").len(), 1);
    }

    #[test]
    fn library_dir_prefers_a_candidate_with_entry_libraries() {
        let tmp = std::env::temp_dir().join("opengdroid_lib_candidates_probe");
        let _ = std::fs::remove_dir_all(&tmp);
        let empty = tmp.join("app-lib");
        let imported = tmp.join("imported-lib");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&imported).unwrap();
        for lib in ENTRY_LIBRARIES {
            std::fs::write(imported.join(lib), b"stub").unwrap();
        }

        // Built directly: Windows temp paths contain a drive colon, which the
        // Android-style separator would split on.
        let candidates = vec![empty.clone(), imported.clone()];
        assert_eq!(pick_library_dir(&candidates), imported);

        // With nothing usable, the first candidate is reported.
        assert_eq!(pick_library_dir(std::slice::from_ref(&empty)), empty);

        // And that candidate makes the launch ready.
        let game = tmp.join("garrysmod");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("gameinfo.txt"), "// test").unwrap();
        let mut req = request(&tmp);
        req.library_candidates = candidates;
        let readiness = check_readiness(&req);
        assert!(readiness.ready(), "imported libs satisfy the entry set");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
