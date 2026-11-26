//! Boundary-neutral result formatting.
//!
//! Both the C ABI ([`crate::ffi`]) and the JNI bridge ([`crate::jni_bridge`])
//! return the same JSON shapes, so the formatting lives here once.

use std::path::Path;

use crate::gma;
use crate::renderer;

/// Escapes a string for embedding in a JSON string literal.
pub fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Resolves the renderer for this launch and reports the decision as JSON.
///
/// Shape: `{"vulkan":bool,"notice":string|null}`. May write the fallback
/// marker, exactly like the original launcher.
pub fn renderer_launch_json(files_dir: &Path, requested: bool, device_version: i32) -> String {
    let launch = renderer::prepare_launch(files_dir, requested, device_version);
    let notice = match &launch.notice {
        Some(n) => format!("\"{}\"", json_escape(n)),
        None => "null".to_owned(),
    };
    format!("{{\"vulkan\":{},\"notice\":{}}}", launch.vulkan, notice)
}

/// Reports a GMA archive's metadata as JSON, or `None` when it cannot be parsed.
///
/// Shape: `{"version":u8,"title":string,"author":string,"description":string,
/// "addon_version":u32,"files":u32,"allowed":u32,"rejected":u32}`.
pub fn gma_info_json(bytes: &[u8]) -> Option<String> {
    let archive = gma::parse(bytes).ok()?;
    let (allowed, rejected) = gma::partition_paths(&archive);
    Some(format!(
        "{{\"version\":{},\"title\":\"{}\",\"author\":\"{}\",\"description\":\"{}\",\"addon_version\":{},\"files\":{},\"allowed\":{},\"rejected\":{}}}",
        archive.version,
        json_escape(&archive.title),
        json_escape(&archive.author),
        json_escape(&archive.description),
        archive.addon_version,
        archive.entries.len(),
        allowed.len(),
        rejected.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_control_and_quote_characters() {
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\u{1}b"), "a\\u0001b");
        assert_eq!(json_escape("обычный"), "обычный");
    }

    #[test]
    fn gma_info_rejects_non_archive_input() {
        assert!(gma_info_json(b"not an addon").is_none());
    }

    #[test]
    fn renderer_json_is_well_formed() {
        let json = renderer_launch_json(Path::new("/nonexistent-dir-for-tests"), false, 0);
        assert!(json.starts_with('{') && json.ends_with('}'));
        assert!(json.contains("\"vulkan\":false"));
        assert!(json.contains("\"notice\":null"));
    }
}
