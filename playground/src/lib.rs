//! Stateless JSON request/response entrypoint for the `omnicoach` WebAssembly playground.
//!
//! Exposes a linear-memory byte buffer exchange (`prepare_input`, `execute_request`,
//! `output_pointer`) over `wasm32-unknown-unknown` with zero external JS-binding crates,
//! alongside the pure-Rust [`handle_request`] function used by tests.

use std::path::Path;
use std::sync::Mutex;

use omni::code_lint;
use omni::diagnostic::{self, Diagnostic, Language};
use omni::rule_catalog;
use omni::rule_selection;
use serde::{Deserialize, Serialize};

static INPUT_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static OUTPUT_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

const FALLBACK_SERIALIZATION_ERROR: &str =
    r#"{"ok":false,"error":"internal serialization failure"}"#;

pub use wasm_abi::{execute_request, output_pointer, prepare_input};

#[expect(
    unsafe_code,
    reason = "WASM cdylib exports require #[unsafe(no_mangle)] in Edition 2024"
)]
mod wasm_abi {
    use super::{INPUT_BUFFER, OUTPUT_BUFFER, execute_buffered_request};

    /// Resizes the WASM input buffer to `byte_length` bytes and returns its linear-memory pointer.
    #[unsafe(no_mangle)]
    pub extern "C" fn prepare_input(byte_length: usize) -> *mut u8 {
        let mut guard = INPUT_BUFFER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.resize(byte_length, 0);
        guard.as_mut_ptr()
    }

    /// Executes the UTF-8 JSON request currently in the input buffer, stores the UTF-8 JSON
    /// response in the output buffer, and returns the response byte length.
    #[unsafe(no_mangle)]
    pub extern "C" fn execute_request() -> usize {
        execute_buffered_request()
    }

    /// Returns the linear-memory pointer to the most recent UTF-8 JSON response buffer.
    #[unsafe(no_mangle)]
    pub extern "C" fn output_pointer() -> *const u8 {
        let guard = OUTPUT_BUFFER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.as_ptr()
    }
}

/// Processes a JSON-encoded `PlaygroundRequest` and returns a JSON-encoded `PlaygroundResponse`.
#[must_use]
pub fn handle_request(request_json: &str) -> String {
    let response = match serde_json::from_str::<PlaygroundRequest>(request_json) {
        Ok(request) => dispatch_request(request),
        Err(parse_error) => PlaygroundResponse::Error {
            ok: false,
            error: format!("invalid JSON request: {parse_error}"),
        },
    };
    serialize_response(&response)
}

fn execute_buffered_request() -> usize {
    let response_json = {
        let input_guard = INPUT_BUFFER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match std::str::from_utf8(&input_guard) {
            Ok(request_text) => handle_request(request_text),
            Err(utf8_error) => serialize_response(&PlaygroundResponse::Error {
                ok: false,
                error: format!("invalid UTF-8 request payload: {utf8_error}"),
            }),
        }
    };
    let mut output_guard = OUTPUT_BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    output_guard.clear();
    output_guard.extend_from_slice(response_json.as_bytes());
    output_guard.len()
}

fn dispatch_request(request: PlaygroundRequest) -> PlaygroundResponse {
    match request {
        PlaygroundRequest::Lint(payload) => run_lint(&payload),
        PlaygroundRequest::Explain { rule, config } => run_explain(&rule, &config),
        PlaygroundRequest::Catalog => run_catalog(),
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
enum PlaygroundRequest {
    /// Lint a single virtual file path with optional `.omnicoach.toml` configuration.
    #[serde(alias = "code")]
    Lint(LintPayload),
    /// Render `--explain <rule>` Markdown evaluated against optional `.omnicoach.toml` configuration.
    Explain {
        rule: String,
        #[serde(default)]
        config: String,
    },
    /// Return the catalog of registered rules for UI discovery.
    Catalog,
}

fn run_lint(payload: &LintPayload) -> PlaygroundResponse {
    let parsed_config = match rule_selection::parse_config(&payload.config) {
        Ok(parsed) => parsed,
        Err(config_error) => {
            return PlaygroundResponse::Error {
                ok: false,
                error: config_error.to_string(),
            };
        }
    };
    let virtual_path = Path::new(&payload.path);
    let language = Language::from_path(virtual_path).map(Language::as_str);
    let is_test_path = parsed_config.is_test_path(virtual_path);
    let diagnostics = code_lint::runner::lint_file(virtual_path, &payload.code, &parsed_config);
    let plain = diagnostic::format_plain(&diagnostics);
    PlaygroundResponse::Lint {
        ok: true,
        language,
        is_test_path,
        diagnostics,
        plain,
    }
}

#[derive(Deserialize)]
struct LintPayload {
    path: String,
    code: String,
    #[serde(default)]
    config: String,
}

fn run_explain(rule: &str, config: &str) -> PlaygroundResponse {
    match rule_catalog::explain_with_config(rule, config) {
        Ok(markdown) => PlaygroundResponse::Explain {
            ok: true,
            rule: rule.to_owned(),
            markdown,
        },
        Err(catalog_error) => PlaygroundResponse::Error {
            ok: false,
            error: catalog_error.to_string(),
        },
    }
}

fn run_catalog() -> PlaygroundResponse {
    let mut rules: Vec<CatalogEntry> = rule_selection::registered_rules()
        .iter()
        .filter(|rule| !rule.languages.is_empty())
        .map(|rule| {
            let languages = rule
                .languages
                .iter()
                .copied()
                .map(Language::as_str)
                .collect();
            CatalogEntry {
                name: rule.name.0,
                languages,
                summary: rule.doc.summary,
            }
        })
        .collect();
    rules.sort_by_key(|entry| entry.name);
    PlaygroundResponse::Catalog { ok: true, rules }
}

fn serialize_response(response: &PlaygroundResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|_| FALLBACK_SERIALIZATION_ERROR.to_owned())
}

#[derive(Serialize)]
#[serde(untagged)]
enum PlaygroundResponse {
    Lint {
        ok: bool,
        language: Option<&'static str>,
        is_test_path: bool,
        diagnostics: Vec<Diagnostic>,
        plain: String,
    },
    Explain {
        ok: bool,
        rule: String,
        markdown: String,
    },
    Catalog {
        ok: bool,
        rules: Vec<CatalogEntry>,
    },
    Error {
        ok: bool,
        error: String,
    },
}

#[derive(Serialize)]
struct CatalogEntry {
    name: &'static str,
    languages: Vec<&'static str>,
    summary: &'static str,
}

#[cfg(test)]
mod tests {
    use super::handle_request;

    #[test]
    fn lint_action_returns_diagnostics_for_python_test_path() -> serde_json::Result<()> {
        let response = handle_request(
            r#"{"action":"lint","path":"tests/test_order.py","code":"import time\n\ndef test_order():\n    time.sleep(1)\n","config":""}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        assert_eq!(parsed["language"].as_str(), Some("python"));
        assert_eq!(parsed["is_test_path"].as_bool(), Some(true));
        assert_eq!(
            parsed["diagnostics"][0]["rule_name"].as_str(),
            Some("sleep-in-tests")
        );
        assert!(
            parsed["plain"]
                .as_str()
                .unwrap_or_default()
                .contains("[sleep-in-tests]")
        );
        Ok(())
    }

    #[test]
    fn lint_action_returns_diagnostics_for_rust_source_path() -> serde_json::Result<()> {
        let response = handle_request(
            r#"{"action":"lint","path":"src/lib.rs","code":"pub fn compute(price_list: &Vec<u64>) -> u64 { price_list.len() as u64 }\n","config":""}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        assert_eq!(parsed["ok"].as_bool(), Some(true));
        assert_eq!(parsed["language"].as_str(), Some("rust"));
        assert_eq!(parsed["is_test_path"].as_bool(), Some(false));
        assert_eq!(
            parsed["diagnostics"][0]["rule_name"].as_str(),
            Some("type-suffixed-name")
        );
        Ok(())
    }

    #[test]
    fn lint_action_respects_toml_ignore_config() -> serde_json::Result<()> {
        let response = handle_request(
            r#"{"action":"lint","path":"tests/test_order.py","code":"import time\n\ndef test_order():\n    time.sleep(1)\n","config":"ignore = [\"sleep-in-tests\"]"}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        assert_eq!(parsed["ok"].as_bool(), Some(true));
        assert_eq!(parsed["diagnostics"].as_array().map(Vec::len), Some(0));
        Ok(())
    }

    #[test]
    fn lint_action_respects_custom_test_patterns_in_config() -> serde_json::Result<()> {
        let response = handle_request(
            r#"{"action":"lint","path":"spec/order.py","code":"import time\n\ndef test_order():\n    time.sleep(1)\n","config":"[context]\ntest-patterns = [\"**/spec/**\"]"}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        assert_eq!(parsed["ok"].as_bool(), Some(true));
        assert_eq!(parsed["is_test_path"].as_bool(), Some(true));
        assert_eq!(
            parsed["diagnostics"][0]["rule_name"].as_str(),
            Some("sleep-in-tests")
        );
        Ok(())
    }

    #[test]
    fn lint_action_reports_invalid_config_selector() -> serde_json::Result<()> {
        let response = handle_request(
            r#"{"action":"lint","path":"src/app.py","code":"x = 1\n","config":"select = [\"tesing\"]"}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        assert_eq!(parsed["ok"], false);
        assert!(
            parsed["error"]
                .as_str()
                .unwrap_or_default()
                .contains("did you mean `testing`?")
        );
        Ok(())
    }

    #[test]
    fn explain_action_renders_rule_markdown_with_live_config_status() -> serde_json::Result<()> {
        let response = handle_request(
            r#"{"action":"explain","rule":"sleep-in-tests","config":"ignore = [\"testing\"]"}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        let markdown = parsed["markdown"].as_str().unwrap_or_default();
        assert_eq!(parsed["ok"], true);
        assert!(markdown.contains("# sleep-in-tests"));
        assert!(markdown.contains("Status: disabled by `ignore = [\"testing\"]`"));
        Ok(())
    }
}
