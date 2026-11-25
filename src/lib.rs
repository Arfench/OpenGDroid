//! OpenGDroid — Rust core for an open reimplementation of the GDroid launcher.
//!
//! This crate provides the platform-independent logic that the Android launcher
//! (and later a full APK) builds on: storage path resolution, Source engine
//! launch-argument assembly, and localization-file parsing.

pub mod ffi;
pub mod gma;
pub mod launch;
pub mod localize;
pub mod renderer;
pub mod storage;

/// Version string reported to the Android layer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }
}
