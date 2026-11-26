//! JNI bridge for the Android front-end.
//!
//! Thin wrapper over the same logic the C ABI exposes ([`crate::ffi`]); both
//! share the JSON formatting in [`crate::api`], so a Kotlin caller and a C
//! caller always see identical results.
//!
//! The Java/Kotlin side declares these as `external` members of
//! `com.opengdroid.core.Core`; see `android/Core.kt`.

use std::path::Path;

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring};
use jni::JNIEnv;

use crate::api;
use crate::gma;
use crate::launch::{self, Extras, Mode, Renderer};
use crate::localize;
use crate::renderer;
use crate::storage;
use crate::VERSION;

/// Copies a Rust string into a Java string, or null if allocation fails.
fn to_jstring(env: &mut JNIEnv, value: &str) -> jstring {
    match env.new_string(value) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn opt_jstring(env: &mut JNIEnv, value: Option<String>) -> jstring {
    match value {
        Some(v) => to_jstring(env, &v),
        None => std::ptr::null_mut(),
    }
}

/// Reads a Java string, returning `None` for null or invalid UTF-16.
fn read_string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    let s = env.get_string(value).ok()?;
    Some(String::from(s))
}

fn read_bytes(env: &mut JNIEnv, value: &JByteArray) -> Option<Vec<u8>> {
    env.convert_byte_array(value).ok()
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_version<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    to_jstring(&mut env, VERSION)
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_sharedRoot<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    external_root: JString<'local>,
) -> jstring {
    let Some(root) = read_string(&mut env, &external_root) else {
        return std::ptr::null_mut();
    };
    let path = storage::shared_root(Path::new(&root));
    to_jstring(&mut env, &path.to_string_lossy())
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_buildArgs<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    game_dir: JString<'local>,
    width: jint,
    height: jint,
    fullscreen: jboolean,
    heap_mb: jint,
    renderer_id: jint,
    extras: JString<'local>,
) -> jstring {
    let Some(game_dir) = read_string(&mut env, &game_dir) else {
        return std::ptr::null_mut();
    };
    let extras_raw = read_string(&mut env, &extras).unwrap_or_default();

    let renderer = if renderer_id == 1 {
        Renderer::Vulkan
    } else {
        Renderer::OpenGlEs
    };

    let args = launch::build(
        &game_dir,
        Mode::new(width.max(0) as u32, height.max(0) as u32, fullscreen != 0),
        heap_mb.max(0) as u32,
        renderer,
        &Extras::parse(&extras_raw),
    );
    to_jstring(&mut env, &args.join(" "))
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_localizationCount<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bytes: JByteArray<'local>,
) -> jlong {
    let Some(payload) = read_bytes(&mut env, &bytes) else {
        return -1;
    };
    match localize::parse_file(&payload) {
        Some((_, entries)) => entries.len() as jlong,
        None => -1,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_localizationLookup<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bytes: JByteArray<'local>,
    key: JString<'local>,
) -> jstring {
    let Some(payload) = read_bytes(&mut env, &bytes) else {
        return std::ptr::null_mut();
    };
    let Some(key) = read_string(&mut env, &key) else {
        return std::ptr::null_mut();
    };
    let Some((_, entries)) = localize::parse_file(&payload) else {
        return std::ptr::null_mut();
    };
    let found = entries
        .iter()
        .find(|e| e.key == key)
        .map(|e| e.value.clone());
    opt_jstring(&mut env, found)
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_rendererLaunch<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
    requested: jboolean,
    device_version: jint,
) -> jstring {
    let Some(dir) = read_string(&mut env, &files_dir) else {
        return std::ptr::null_mut();
    };
    let json = api::renderer_launch_json(Path::new(&dir), requested != 0, device_version);
    to_jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_rendererClear<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
) -> jint {
    let Some(dir) = read_string(&mut env, &files_dir) else {
        return -1;
    };
    match renderer::clear_fallback(Path::new(&dir)) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_gmaCount<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bytes: JByteArray<'local>,
) -> jlong {
    let Some(payload) = read_bytes(&mut env, &bytes) else {
        return -1;
    };
    match gma::parse(&payload) {
        Ok(archive) => archive.entries.len() as jlong,
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_gmaInfo<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bytes: JByteArray<'local>,
) -> jstring {
    let Some(payload) = read_bytes(&mut env, &bytes) else {
        return std::ptr::null_mut();
    };
    opt_jstring(&mut env, api::gma_info_json(&payload))
}

#[no_mangle]
pub extern "system" fn Java_com_opengdroid_core_Core_gmaPathAllowed<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jint {
    let Some(path) = read_string(&mut env, &path) else {
        return -1;
    };
    i32::from(gma::is_allowed_path(&path))
}
