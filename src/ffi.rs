//! C ABI surface for the Android layer.
//!
//! The Android front-end (Kotlin/Java via JNI, or C) calls these functions. All
//! strings cross the boundary as NUL-terminated UTF-8; anything returned must be
//! released with [`opengdroid_string_free`].

use std::ffi::{c_char, CStr, CString};
use std::path::Path;

use crate::launch::{self, Extras, Mode, Renderer};
use crate::localize;
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
