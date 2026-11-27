# OpenGDroid

Open-source Rust reimplementation of the [GDroid](https://github.com/tinsarfal/GDroid) Android
launcher — the unofficial Source-engine / Garry's Mod port for Android.

The goal is a clean-room Rust core with an Android front-end, built as an APK, that keeps
compatibility with the existing GDroid content layout and launch arguments.

## Status

Early stage. The Rust core is functional and unit-tested; the Android front-end is not built yet.

| Module | What it does | Status |
|---|---|---|
| `storage` | Resolves game-content paths on external storage, mirroring GDroid's search order | done, tested |
| `launch` | Builds the Source engine command line (renderer, resolution, heap, user extras) | done, tested |
| `localize` | Parses Source localization tables, **including UTF-16LE** | done, tested |

## Why this exists

Analysis of the original 0.1.31 build found a set of concrete, fixable issues that this project
addresses by design:

1. **Localization is broken.** Every `*_english.txt` file is UTF-16LE, but the parser reads it as
   UTF-8, so all 11 tables load with zero entries. `localize` detects the BOM and decodes
   correctly, so translated strings actually appear.
2. **Renderer selection is manual.** Vulkan is only used if the user types
   `-sourcevr_native_vulkan` into the launch arguments. Here the choice is modelled in the type
   system (`Renderer`) and can be resolved automatically from device capability.
3. **Steam's C API is a stub.** `libsteam_api.so` in the original is an 8.9 KB shim; the real
   authentication lives in the Java layer. This project keeps that boundary explicit rather than
   pretending the C API works.
4. **Storage policy silently blocks writes.** Paths like `cfg/touch.cfg` are rejected at runtime.
   `storage` centralises path resolution so these cases can be handled in one place.

## Layout

```
src/
  lib.rs        crate root, version
  storage.rs    external-storage path resolution
  launch.rs     engine argument + config generation
  localize.rs   localization parsing with encoding detection
  renderer.rs   Vulkan/GLES selection with crash-marker fallback
  gma.rs        Garry's Mod addon archive parsing + content whitelist
  ffi.rs        C ABI surface for the Android layer
.cargo/
  config.toml   Android cross-compilation (NDK clang)
tests/
  device_smoke.c     on-device smoke test for the built .so
  device_real_file.c parses a real UTF-16LE table from the device
  device_features.c  renderer selection + GMA parsing on device
```

## C ABI

The library builds as a `cdylib` exposing a small C surface, so the Android front-end can call it
through JNI or directly from C:

| Function | Purpose |
|---|---|
| `opengdroid_version()` | Static version string (do not free) |
| `opengdroid_shared_root(external_root)` | Resolved content root, caller frees |
| `opengdroid_build_args(game_dir, w, h, fullscreen, heap_mb, renderer, extras)` | Engine command line, caller frees |
| `opengdroid_localization_count(bytes, len)` | Number of entries, `-1` on decode failure |
| `opengdroid_localization_lookup(bytes, len, key)` | Value for key, caller frees, null if absent |
| `opengdroid_renderer_launch(files_dir, requested, device_version)` | JSON `{"vulkan":bool,"notice":...}`, caller frees |
| `opengdroid_renderer_clear(files_dir)` | Clears the fallback marker: `0` ok, `-1` bad arg, `-2` I/O |
| `opengdroid_gma_count(bytes, len)` | Files in a `.gma`, `-1` if not a plain archive |
| `opengdroid_gma_info(bytes, len)` | JSON metadata incl. allowed/rejected counts |
| `opengdroid_gma_path_allowed(path)` | `1` allowed, `0` rejected, `-1` bad arg |
| `opengdroid_string_free(ptr)` | Releases any string returned above |

All strings cross the boundary as NUL-terminated UTF-8. `renderer` is `1` for Vulkan, anything
else for OpenGL ES.

## Verified on device

Built for `aarch64-linux-android` and `armv7-linux-androideabi` with NDK r27c, then loaded on a
Samsung SM-S711B (Android 14, arm64-v8a). `tests/device_smoke.c` dlopen'd the library and checked
every function; all assertions passed, including:

```
root = /storage/emulated/0/srceng
args = -game garrysmod -steam ... -w 1520 -h 720 +exec android_settings.cfg -sourcevr_native_vulkan +map gm_construct
lookup(A) = Alpha          # from a UTF-16LE table the original launcher parses as zero entries
RESULT: ALL PASS
```

## Building

Requirements: Rust stable, Android NDK, `aarch64-linux-android` / `armv7-linux-androideabi` targets.

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi
cargo test                     # host tests
cargo build --release --target aarch64-linux-android
```

Adjust the NDK paths in `.cargo/config.toml` to your installation.

## Testing

```bash
cargo test
```

Unit tests cover BOM detection, UTF-16LE decoding (including the case the original launcher
fails), argument assembly, renderer token detection, extras parsing, and path resolution.

## Compatibility notes

Content layout matches GDroid, so an existing install keeps working:

- Content root: first existing of `/sdcard/srceng`, `Download/srceng`, emulator shared folders.
- Game content is often a symlink: `<app files>/custom-content-garrysmod` →
  `/sdcard/Download/Garry's Mod`.
- Launch arguments are read from the `launch_args` setting and passed through verbatim, so
  `+map`, `+connect` and `-sourcevr_native_vulkan` behave as before.

## Roadmap

### Done

- Rust core: `storage`, `launch`, `launch_plan`, `engine`, `localize` (UTF-16LE fixed), `renderer`, `gma` — 60 host tests, clippy clean with `-D warnings`.
- Android cross-build for `aarch64-linux-android` and `armv7-linux-androideabi` via NDK r27c.
- C ABI with 13 exported functions, verified on SM-S711B (Android 14, arm64-v8a): all assertions pass.
- JNI bridge (`src/jni_bridge.rs` → `com.opengdroid.core.Core`), verified on device via `app_process`.
- Minimal Android app built and installed: shows content root, engine command line, renderer decision, addon counts and the launch plan; writes `android_settings.cfg` into the real game folder.
- `storage` prefers a directory that actually contains a game (`garrysmod/gameinfo.txt`) over an empty `srceng` stub, so `Download/Garry's Mod` is found.
- `renderer`: Vulkan/GLES selection with crash-marker fallback matches the original protocol.
- `gma`: addon archive parsing plus content whitelist (engine libraries and traversal rejected).
- `launch_plan`: the full contract the engine is started with — the fifteen environment variables the original launcher sets (`APP_DATA_PATH`, `APP_LIB_PATH`, `SOURCEVR_GAME`, `EXTRAS_VPK_PATH`, cache and TMPDIR variables), the argument vector, and a readiness check that names what is missing instead of failing silently.
- `engine`: the entry libraries are actually loaded with `dlopen`, and bionic's first-missing-dependency errors are resolved by mapping the named dependency with `RTLD_GLOBAL` and retrying. Verified on device: both entry libraries load and `LauncherMainAndroid` is found.

### Not done

- The app does not call `LauncherMainAndroid` yet — the symbol is located and reported, but the engine is not entered, because doing so takes over the process (SDL owns the surface and the main loop) and needs its own activity.
- No settings UI for launch arguments, resolution or heap size (values are fixed in `MainActivity`).
- No localization UI: `localize` is exposed and tested, but no screen reads the tables yet.
- `armv7` build not exercised on hardware — only the arm64 `.so` was loaded on a device.
- No Gradle project: the APK is assembled by `android/build.sh` (aapt2 + d8 + apksigner).
- No addon management UI; `gma` is reachable only through the native API.
- Engine libraries are used from the installed GDroid package rather than bundled; a standalone build needs its own copy of them.
- Steam integration stays an explicit boundary, as in the original.

### Plans

1. Enter the engine: a dedicated activity that loads the entry libraries, calls `LauncherMainAndroid` with the prepared `argv` and lets SDL own the surface.
2. Settings screen: launch arguments, resolution, heap size, renderer — persisted and fed into the plan.
3. Localization UI backed by `localize`, so translated strings appear (the original bug this project fixes).
4. Addon management UI backed by `gma` parsing (list, validate, enable/disable).
5. CI: `cargo test` + clippy on host, cross-build for both Android targets, APK artifact.
6. Verify the `armv7` `.so` on a 32-bit device or emulator.
7. Replace `build.sh` with a Gradle project once the app outgrows the manual pipeline.

## License

MIT — see [LICENSE](LICENSE).
