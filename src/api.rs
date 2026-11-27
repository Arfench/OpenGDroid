//! Boundary-neutral result formatting.
//!
//! Both the C ABI ([`crate::ffi`]) and the JNI bridge ([`crate::jni_bridge`])
//! return the same JSON shapes, so the formatting lives here once.

use std::path::Path;

use crate::engine::Probe;
use crate::gma;
use crate::launch_plan::{Plan, Readiness};
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

/// Reports a launch plan as JSON: the command line, the environment and what
/// still blocks execution.
///
/// Shape: `{"ready":bool,"game_installed":bool,"blockers":[string],
/// "missing_entry_libraries":[string],"missing_engine_libraries":[string],
/// "library_dir":string,"command":string,"args":[string],"env":[string]}`.
pub fn launch_plan_json(plan: &Plan) -> String {
    format!(
        "{{\"ready\":{},\"game_installed\":{},\"blockers\":{},\"missing_entry_libraries\":{},\
         \"missing_engine_libraries\":{},\"library_dir\":\"{}\",\"command\":\"{}\",\"args\":{},\"env\":{}}}",
        plan.ready(),
        plan.readiness.game_installed,
        json_string_array(&plan.readiness.blockers()),
        json_string_array(&plan.readiness.missing_entry_libraries),
        json_string_array(&plan.readiness.missing_engine_libraries),
        json_escape(&plan.library_dir.to_string_lossy()),
        json_escape(&plan.command_line()),
        json_string_array(&plan.args),
        json_string_array(&plan.env_pairs())
    )
}

/// Renders a slice of strings as a JSON array literal.
pub fn json_string_array(items: &[String]) -> String {
    let inner: Vec<String> = items
        .iter()
        .map(|item| format!("\"{}\"", json_escape(item)))
        .collect();
    format!("[{}]", inner.join(","))
}

/// Reports readiness alone, for callers that only need the go/no-go decision.
///
/// Shape: `{"ready":bool,"blockers":[string]}`.
pub fn readiness_json(readiness: &Readiness) -> String {
    format!(
        "{{\"ready\":{},\"blockers\":{}}}",
        readiness.ready(),
        json_string_array(&readiness.blockers())
    )
}

/// Reports an engine library probe as JSON.
///
/// Shape: `{"all_loaded":bool,"entry_symbol":bool,"failed":[string],
/// "errors":[string],"libraries":[{"name":string,"loaded":bool,
/// "error":string|null,"entry_symbol":bool}]}`.
pub fn engine_probe_json(probe: &Probe) -> String {
    let libraries: Vec<String> = probe
        .libraries
        .iter()
        .map(|lib| {
            let error = match &lib.error {
                Some(e) => format!("\"{}\"", json_escape(e)),
                None => "null".to_owned(),
            };
            format!(
                "{{\"name\":\"{}\",\"loaded\":{},\"error\":{},\"entry_symbol\":{}}}",
                json_escape(&lib.name),
                lib.loaded,
                error,
                lib.has_entry_symbol
            )
        })
        .collect();

    format!(
        "{{\"all_loaded\":{},\"entry_symbol\":{},\"failed\":{},\"errors\":{},\"loaded\":{},\"libraries\":[{}]}}",
        probe.all_loaded(),
        probe.entry_symbol_found(),
        json_string_array(&probe.failed()),
        json_string_array(&probe.errors()),
        json_string_array(&probe.loaded),
        libraries.join(",")
    )
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

    #[test]
    fn string_array_escapes_and_joins() {
        assert_eq!(json_string_array(&[]), "[]");
        assert_eq!(
            json_string_array(&["a".to_owned(), "b\"c".to_owned()]),
            "[\"a\",\"b\\\"c\"]"
        );
    }

    #[test]
    fn launch_plan_json_reports_blockers() {
        use crate::launch::{Extras, Mode, Renderer};
        use crate::launch_plan::{plan, Request};

        let root = Path::new("/nonexistent-probe-root");
        let request = Request {
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
            extras: Extras::parse(""),
        };

        let json = launch_plan_json(&plan(&request));
        assert!(json.starts_with('{') && json.ends_with('}'));
        assert!(json.contains("\"ready\":false"));
        assert!(json.contains("\"game_installed\":false"));
        assert!(json.contains("gameinfo.txt"));
        assert!(json.contains("\"command\":\"-game "));
        assert!(json.contains("SOURCEVR_GAME=garrysmod"));

        let readiness = readiness_json(&plan(&request).readiness);
        assert!(readiness.contains("\"ready\":false"));
        assert!(readiness.contains("gameinfo.txt"));
    }
}
