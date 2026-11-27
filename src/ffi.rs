//! C ABI surface for the Android layer.
//!
//! The Android front-end (Kotlin/Java via JNI, or C) calls these functions. All
//! strings cross the boundary as NUL-terminated UTF-8; anything returned must be
//! released with [`opengdroid_string_free`].

use std::ffi::{c_char, CStr, CString};
use std::path::Path;

use crate::api;
use crate::engine;
use crate::gma;
use crate::launch::{self, Extras, Mode, Renderer};
use crate::launch_plan::{self, Request};
use crate::localize;
use crate::renderer;
use crate::storage;

/// Returns the crate version as a static NUL-terminated string.
///
/// The returned pointer is static and must **not** be freed.
#[no_mangle]
pub extern "C" fn opengdroid_version() -> *const c_char {
    // The literal is NUL-terminated at compile time; no allocation involved.
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Resolves the shared content root beneath `external_root`.
///
/// Returns a heap string to free with [`opengdroid_string_free`], or null when
/// `external_root` is not valid UTF-8 or contains a NUL byte.
///
/// # Safety
/// `external_root` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_shared_root(external_root: *const c_char) -> *mut c_char {
    let Some(root) = (unsafe { cstr_to_str(external_root) }) else {
        return std::ptr::null_mut();
    };
    let path = storage::shared_root(Path::new(root));
    to_c_string(&path.to_string_lossy())
}

/// Builds the engine command line and returns it as a single space-separated
/// string, or null on invalid input.
///
/// `renderer` is 1 for Vulkan and anything else for OpenGL ES.
///
/// # Safety
/// All pointer arguments must be null or valid NUL-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_build_args(
    game_dir: *const c_char,
    width: u32,
    height: u32,
    fullscreen: bool,
    heap_mb: u32,
    renderer: u32,
    extras: *const c_char,
) -> *mut c_char {
    let Some(game_dir) = (unsafe { cstr_to_str(game_dir) }) else {
        return std::ptr::null_mut();
    };
    let extras_raw = unsafe { cstr_to_str(extras) }.unwrap_or_default();

    let renderer = if renderer == 1 {
        Renderer::Vulkan
    } else {
        Renderer::OpenGlEs
    };

    let args = launch::build(
        game_dir,
        Mode::new(width, height, fullscreen),
        heap_mb,
        renderer,
        &Extras::parse(extras_raw),
    );
    to_c_string(&args.join(" "))
}

/// Builds the full launch plan and returns it as JSON.
///
/// Shape: `{"ready":bool,"game_installed":bool,"blockers":[string],
/// "missing_entry_libraries":[string],"missing_engine_libraries":[string],
/// "command":string,"args":[string],"env":[string]}`.
///
/// `renderer` is 1 for Vulkan and anything else for OpenGL ES.
///
/// # Safety
/// All pointer arguments must be null or valid NUL-terminated C strings.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn opengdroid_launch_plan(
    shared_root: *const c_char,
    game_id: *const c_char,
    native_lib_dir: *const c_char,
    user_dir: *const c_char,
    engine_content: *const c_char,
    runtime_vpk: *const c_char,
    cache_dir: *const c_char,
    width: u32,
    height: u32,
    fullscreen: bool,
    heap_mb: u32,
    renderer: u32,
    extras: *const c_char,
) -> *mut c_char {
    let Some(shared_root) = (unsafe { cstr_to_str(shared_root) }) else {
        return std::ptr::null_mut();
    };
    let game_id = unsafe { cstr_to_str(game_id) }.unwrap_or("garrysmod");
    let native_lib_dir = unsafe { cstr_to_str(native_lib_dir) }.unwrap_or_default();
    let user_dir = unsafe { cstr_to_str(user_dir) }.unwrap_or_default();
    let engine_content = unsafe { cstr_to_str(engine_content) }.unwrap_or_default();
    let runtime_vpk = unsafe { cstr_to_str(runtime_vpk) }.unwrap_or_default();
    let cache_dir = unsafe { cstr_to_str(cache_dir) }.unwrap_or_default();
    let extras_raw = unsafe { cstr_to_str(extras) }.unwrap_or_default();

    let request = Request {
        shared_root: Path::new(shared_root).to_path_buf(),
        game_id: game_id.to_owned(),
        library_candidates: launch_plan::library_candidates(native_lib_dir),
        user_dir: Path::new(user_dir).to_path_buf(),
        engine_content: Path::new(engine_content).to_path_buf(),
        runtime_vpk: Path::new(runtime_vpk).to_path_buf(),
        cache_dir: Path::new(cache_dir).to_path_buf(),
        mode: Mode::new(width, height, fullscreen),
        heap_mb,
        renderer: if renderer == 1 {
            Renderer::Vulkan
        } else {
            Renderer::OpenGlEs
        },
        extras: Extras::parse(extras_raw),
    };

    to_c_string(&api::launch_plan_json(&launch_plan::plan(&request)))
}

/// Probes the engine entry libraries by actually loading them with `dlopen`.
///
/// Returns JSON: `{"all_loaded":bool,"entry_symbol":bool,"failed":[string],
/// "errors":[string],"libraries":[...]}`. Libraries are unloaded again, so this
/// has no side effect on the process.
///
/// # Safety
/// `library_dir` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_engine_probe(library_dir: *const c_char) -> *mut c_char {
    let Some(dir) = (unsafe { cstr_to_str(library_dir) }) else {
        return std::ptr::null_mut();
    };
    to_c_string(&api::engine_probe_json(&engine::probe(Path::new(dir))))
}

/// Counts the localization entries in `bytes`.
///
/// Returns the number of `"Key" "Value"` pairs found, or `-1` when the payload
/// cannot be decoded (unknown encoding or truncated UTF-16).
///
/// # Safety
/// `bytes` must point to at least `len` readable bytes, or be null with `len` 0.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_localization_count(bytes: *const u8, len: usize) -> i64 {
    let Some(slice) = (unsafe { slice_from_raw(bytes, len) }) else {
        return -1;
    };
    match localize::parse_file(slice) {
        Some((_, entries)) => entries.len() as i64,
        None => -1,
    }
}

/// Looks up a localization value by key, returning a heap string or null when
/// the payload cannot be decoded or the key is absent.
///
/// # Safety
/// `bytes` must point to at least `len` readable bytes; `key` must be null or a
/// valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_localization_lookup(
    bytes: *const u8,
    len: usize,
    key: *const c_char,
) -> *mut c_char {
    let Some(slice) = (unsafe { slice_from_raw(bytes, len) }) else {
        return std::ptr::null_mut();
    };
    let Some(key) = (unsafe { cstr_to_str(key) }) else {
        return std::ptr::null_mut();
    };
    let Some((_, entries)) = localize::parse_file(slice) else {
        return std::ptr::null_mut();
    };
    match entries.iter().find(|e| e.key == key) {
        Some(entry) => to_c_string(&entry.value),
        None => std::ptr::null_mut(),
    }
}

/// Frees a string previously returned by this library.
///
/// Passing null is a no-op.
///
/// # Safety
/// `ptr` must come from this library and must not be freed twice.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

/// Returns the renderer decision for this launch as a JSON object.
///
/// Shape: `{"vulkan":bool,"notice":string|null}`. The marker directory is
/// `files_dir/renderer-v1`; `device_version` is
/// `android.hardware.vulkan.version` (`4198400` == Vulkan 1.1), and
/// `requested` is non-zero when the user asked for Vulkan.
///
/// Side effect: may write the fallback marker, exactly like the original.
///
/// # Safety
/// `files_dir` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_renderer_launch(
    files_dir: *const c_char,
    requested: i32,
    device_version: i32,
) -> *mut c_char {
    let Some(dir) = (unsafe { cstr_to_str(files_dir) }) else {
        return std::ptr::null_mut();
    };
    to_c_string(&api::renderer_launch_json(
        Path::new(dir),
        requested != 0,
        device_version,
    ))
}

/// Clears the renderer fallback marker so the user can retry Vulkan.
///
/// Returns 0 on success, `-1` on a null argument, `-2` on an I/O error.
///
/// # Safety
/// `files_dir` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_renderer_clear(files_dir: *const c_char) -> i32 {
    let Some(dir) = (unsafe { cstr_to_str(files_dir) }) else {
        return -1;
    };
    match renderer::clear_fallback(Path::new(dir)) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Counts the entries in a GMA archive held in memory.
///
/// Returns the number of files, or `-1` when the payload is not a plain GMA.
///
/// # Safety
/// `bytes` must point to at least `len` readable bytes, or be null with `len` 0.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_gma_count(bytes: *const u8, len: usize) -> i64 {
    let Some(slice) = (unsafe { slice_from_raw(bytes, len) }) else {
        return -1;
    };
    match gma::parse(slice) {
        Ok(archive) => archive.entries.len() as i64,
        Err(_) => -1,
    }
}

/// Returns a GMA archive's metadata as a JSON object, or null on parse failure.
///
/// Shape: `{"version":u8,"title":string,"author":string,"description":string,
/// "addon_version":u32,"files":u32,"allowed":u32,"rejected":u32}`.
///
/// # Safety
/// `bytes` must point to at least `len` readable bytes, or be null with `len` 0.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_gma_info(bytes: *const u8, len: usize) -> *mut c_char {
    let Some(slice) = (unsafe { slice_from_raw(bytes, len) }) else {
        return std::ptr::null_mut();
    };
    match api::gma_info_json(slice) {
        Some(json) => to_c_string(&json),
        None => std::ptr::null_mut(),
    }
}

/// Checks whether a single path is allowed by the addon content whitelist.
///
/// Returns 1 when allowed, 0 when rejected, `-1` on a null argument.
///
/// # Safety
/// `path` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn opengdroid_gma_path_allowed(path: *const c_char) -> i32 {
    let Some(path) = (unsafe { cstr_to_str(path) }) else {
        return -1;
    };
    i32::from(gma::is_allowed_path(path))
}

/// # Safety
/// `ptr` must be null or a valid NUL-terminated C string.
unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

/// # Safety
/// `ptr` must point to at least `len` readable bytes, or be null with `len` 0.
unsafe fn slice_from_raw<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

fn to_c_string(value: &str) -> *mut c_char {
    match CString::new(value) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_exposed() {
        let ptr = opengdroid_version();
        assert!(!ptr.is_null());
        let version = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(version, crate::VERSION);
    }

    #[test]
    fn shared_root_roundtrip() {
        let root = CString::new("/nonexistent-probe-root").unwrap();
        let ptr = unsafe { opengdroid_shared_root(root.as_ptr()) };
        assert!(!ptr.is_null());
        let value = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        unsafe { opengdroid_string_free(ptr) };
        assert!(value.ends_with("srceng"));
    }

    #[test]
    fn null_input_returns_null() {
        assert!(unsafe { opengdroid_shared_root(std::ptr::null()) }.is_null());
        assert!(unsafe { opengdroid_localization_count(std::ptr::null(), 4) } == -1);
    }

    #[test]
    fn build_args_roundtrip() {
        let game = CString::new("garrysmod").unwrap();
        let extras = CString::new("+map gm_construct").unwrap();
        let ptr = unsafe {
            opengdroid_build_args(game.as_ptr(), 1520, 720, true, 512, 1, extras.as_ptr())
        };
        assert!(!ptr.is_null());
        let joined = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        unsafe { opengdroid_string_free(ptr) };
        assert!(joined.contains("-game garrysmod"));
        assert!(joined.contains("-sourcevr_native_vulkan"));
        assert!(joined.ends_with("+map gm_construct"));
    }

    #[test]
    fn localization_lookup_through_ffi() {
        let payload = b"\"A\" \"Alpha\"\n\"B\" \"Beta\"\n";
        assert_eq!(
            unsafe { opengdroid_localization_count(payload.as_ptr(), payload.len()) },
            2
        );

        let key = CString::new("B").unwrap();
        let ptr = unsafe {
            opengdroid_localization_lookup(payload.as_ptr(), payload.len(), key.as_ptr())
        };
        assert!(!ptr.is_null());
        let value = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        unsafe { opengdroid_string_free(ptr) };
        assert_eq!(value, "Beta");

        let missing = CString::new("Nope").unwrap();
        assert!(unsafe {
            opengdroid_localization_lookup(payload.as_ptr(), payload.len(), missing.as_ptr())
        }
        .is_null());
    }

    #[test]
    fn free_null_is_noop() {
        unsafe { opengdroid_string_free(std::ptr::null_mut()) };
    }
}
