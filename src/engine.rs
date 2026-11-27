//! In-process loading of the engine's entry libraries.
//!
//! The Source modules are not bundled with this app: they live inside the
//! original GDroid APK and are used from there. Being present on disk is not
//! the same as being loadable by *this* process, so the probe below actually
//! calls `dlopen` and reports what the loader said.
//!
//! Bionic resolves `DT_NEEDED` only through the namespace search paths, not
//! through the directory of the library being loaded. Loading `liblauncher.so`
//! from an arbitrary directory therefore fails with
//! `library "libtogl.so" not found: needed by ...` even though the file sits
//! right next to it. [`probe`] reads that message, loads the named dependency
//! by absolute path with `RTLD_GLOBAL`, and retries — so only the libraries the
//! loader actually asks for are mapped, and no search path is guessed.

use std::path::Path;

use crate::launch_plan::ENTRY_LIBRARIES;

/// Symbol SDL calls once the entry libraries are loaded.
pub const ENTRY_SYMBOL: &str = "LauncherMainAndroid";

/// Guard against a pathological dependency cycle in a broken install.
const MAX_DEPENDENCY_DEPTH: usize = 32;

/// Outcome of loading one library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryLoad {
    pub name: String,
    /// True when `dlopen` succeeded.
    pub loaded: bool,
    /// Loader error when `loaded` is false.
    pub error: Option<String>,
    /// True when the library exports [`ENTRY_SYMBOL`].
    pub has_entry_symbol: bool,
}

/// Result of probing every entry library.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    pub libraries: Vec<LibraryLoad>,
    /// Every library the loader mapped, dependencies included.
    pub loaded: Vec<String>,
}

impl Probe {
    /// True when every entry library loaded.
    pub fn all_loaded(&self) -> bool {
        !self.libraries.is_empty() && self.libraries.iter().all(|lib| lib.loaded)
    }

    /// True when some loaded library exports the entry symbol.
    pub fn entry_symbol_found(&self) -> bool {
        self.libraries.iter().any(|lib| lib.has_entry_symbol)
    }

    /// Loader errors, prefixed with the library they came from.
    pub fn errors(&self) -> Vec<String> {
        self.libraries
            .iter()
            .filter_map(|lib| lib.error.as_ref().map(|e| format!("{}: {e}", lib.name)))
            .collect()
    }

    /// Names of the libraries that failed to load.
    pub fn failed(&self) -> Vec<String> {
        self.libraries
            .iter()
            .filter(|lib| !lib.loaded)
            .map(|lib| lib.name.clone())
            .collect()
    }
}

/// Loads each entry library from `dir`, resolving dependencies on demand.
///
/// Libraries stay mapped: dependents need them resident, and the real launch
/// keeps the same set loaded.
pub fn probe(dir: &Path) -> Probe {
    let mut probe = Probe::default();

    for name in ENTRY_LIBRARIES {
        let mut entry = LibraryLoad {
            name: (*name).to_owned(),
            loaded: false,
            error: None,
            has_entry_symbol: false,
        };

        match load_with_dependencies(dir, name, &mut probe.loaded, 0) {
            Ok(handle) => {
                entry.loaded = true;
                entry.has_entry_symbol = sys::sym(handle, ENTRY_SYMBOL).is_some();
            }
            Err(error) => entry.error = Some(error),
        }

        probe.libraries.push(entry);
    }

    probe
}

/// Loads `name` from `dir`, satisfying dependencies until the loader is happy.
///
/// The loader reports only the first missing dependency, so a single retry is
/// not enough: `liblauncher.so` needs `libtogl.so`, then `libsteam_api.so`, and
/// so on. The loop keeps resolving whatever it names until the library loads.
fn load_with_dependencies(
    dir: &Path,
    name: &str,
    loaded: &mut Vec<String>,
    depth: usize,
) -> Result<Handle, String> {
    if depth > MAX_DEPENDENCY_DEPTH {
        return Err(format!("dependency chain for {name} is too deep"));
    }
    if loaded.iter().any(|lib| lib == name) {
        // Already mapped globally: re-open by soname to obtain a handle.
        return sys::open(name);
    }

    let path = dir.join(name);
    let mut attempts = 0;

    loop {
        match sys::open(&path.to_string_lossy()) {
            Ok(handle) => {
                loaded.push(name.to_owned());
                return Ok(handle);
            }
            Err(error) => {
                let Some(dependency) = missing_dependency(&error) else {
                    return Err(error);
                };
                if dependency == name || attempts >= MAX_DEPENDENCY_DEPTH {
                    return Err(error);
                }
                attempts += 1;
                load_with_dependencies(dir, &dependency, loaded, depth + 1)?;
            }
        }
    }
}

/// Extracts the soname from `library "libtogl.so" not found: needed by ...`.
///
/// Returns `None` for any other loader error, so unrelated failures are
/// reported as-is instead of triggering a pointless load attempt.
pub fn missing_dependency(error: &str) -> Option<String> {
    const MARKER: &str = "not found: needed by";
    if !error.contains(MARKER) {
        return None;
    }
    let start = error.find('"')? + 1;
    let rest = &error[start..];
    let end = rest.find('"')?;
    let name = &rest[..end];
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

/// Opaque `dlopen` handle; only its nullness and identity matter here.
pub type Handle = *mut std::ffi::c_void;

#[cfg(target_os = "android")]
mod sys {
    use std::ffi::{c_char, c_int, c_void, CStr, CString};

    use super::Handle;

    // bionic: RTLD_LAZY = 1, RTLD_GLOBAL = 0x00100.
    //
    // RTLD_GLOBAL matters: a dependency loaded locally is not visible to the
    // library that needs it, so the retry would fail again.
    const RTLD_LAZY: c_int = 1;
    const RTLD_GLOBAL: c_int = 0x00100;

    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlerror() -> *mut c_char;
    }

    pub fn open(path: &str) -> Result<Handle, String> {
        let Ok(c_path) = CString::new(path) else {
            return Err("path contains a NUL byte".to_owned());
        };
        let handle = unsafe { dlopen(c_path.as_ptr(), RTLD_LAZY | RTLD_GLOBAL) };
        if handle.is_null() {
            return Err(last_error());
        }
        Ok(handle)
    }

    pub fn sym(handle: Handle, name: &str) -> Option<Handle> {
        let Ok(c_name) = CString::new(name) else {
            return None;
        };
        let ptr = unsafe { dlsym(handle, c_name.as_ptr()) };
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }

    fn last_error() -> String {
        let ptr = unsafe { dlerror() };
        if ptr.is_null() {
            return "unknown loader error".to_owned();
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(not(target_os = "android"))]
mod sys {
    use super::Handle;

    pub fn open(_path: &str) -> Result<Handle, String> {
        Err("dlopen is only available on Android".to_owned())
    }

    pub fn sym(_handle: Handle, _name: &str) -> Option<Handle> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_libraries_are_reported_as_errors() {
        let dir = std::env::temp_dir().join("opengdroid_engine_probe_missing");
        let _ = std::fs::remove_dir_all(&dir);

        let probe = probe(&dir);
        assert_eq!(probe.libraries.len(), ENTRY_LIBRARIES.len());
        assert!(!probe.all_loaded());
        assert!(!probe.entry_symbol_found());
        assert_eq!(probe.failed().len(), ENTRY_LIBRARIES.len());
        assert_eq!(probe.errors().len(), ENTRY_LIBRARIES.len());
        assert!(probe.errors()[0].starts_with(ENTRY_LIBRARIES[0]));
    }

    #[test]
    fn empty_probe_is_not_ready() {
        let probe = Probe::default();
        assert!(!probe.all_loaded(), "no libraries means nothing loaded");
        assert!(probe.errors().is_empty());
    }

    #[test]
    fn dependency_name_is_read_from_loader_error() {
        let error = "dlopen failed: library \"libtogl.so\" not found: needed by \
                     /data/app/lib/arm64/liblauncher.so in namespace clns-4";
        assert_eq!(missing_dependency(error).as_deref(), Some("libtogl.so"));
    }

    #[test]
    fn unrelated_loader_errors_are_not_treated_as_dependencies() {
        assert_eq!(
            missing_dependency("dlopen failed: cannot locate symbol \"foo\""),
            None
        );
        assert_eq!(missing_dependency("dlopen failed: file too short"), None);
        assert_eq!(missing_dependency(""), None);
    }
}
