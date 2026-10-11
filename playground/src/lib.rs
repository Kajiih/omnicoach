//! Stateless JSON request/response entrypoint for the `omnicoach` WebAssembly playground.
//!
//! Exposes a linear-memory byte buffer exchange (`prepare_input`, `execute_request`,
//! `output_pointer`) over `wasm32-unknown-unknown` with zero external JS-binding crates,
//! alongside the pure-Rust [`handle_request`] function used by tests.

use std::path::Path;
use std::sync::Mutex;

use omni::code_lint;
use omni::diagnostic::{self, Diagnostic};
use omni::rule_catalog;
use omni::rule_selection;
use serde::{Deserialize, Serialize};

static INPUT_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static OUTPUT_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

const FALLBACK_SERIALIZATION_ERROR: &str =
    r#"{"ok":false,"error":"internal serialization failure"}"#;

/// Resizes the WASM input buffer to `byte_length` bytes and returns its linear-memory pointer.
#[no_mangle]
pub extern "C" fn prepare_input(byte_length: usize) -> *mut u8 {
    let mut guard = INPUT_BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.resize(byte_length, 0);
    guard.as_mut_ptr()
}

/// Executes the UTF-8 JSON request currently in the input buffer, stores the UTF-8 JSON
/// response in the output buffer, and returns the response byte length.
#[no_mangle]
pub extern "C" fn execute_request() -> usize {
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

/// Returns the linear-memory pointer to the most recent UTF-8 JSON response buffer.
#[no_mangle]
pub extern "C" fn output_pointer() -> *const u8 {
    let guard = OUTPUT_BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.as_ptr()
}

/// Processes a JSON-encoded [`PlaygroundRequest`] and returns a JSON-encoded [`PlaygroundResponse`].
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

#[derive(Deserialize)]
struct LintPayload {
    path: String,
    code: String,
    #[serde(default)]
    config: String,
}

#[derive(Serialize)]
#[serde(untagged)]
enum PlaygroundResponse {
    Lint {
        ok: bool,
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

fn dispatch_request(request: PlaygroundRequest) -> PlaygroundResponse {
    match request {
        PlaygroundRequest::Lint(payload) => run_lint(&payload),
        PlaygroundRequest::Explain { rule, config } => run_explain(&rule, &config),
        PlaygroundRequest::Catalog => run_catalog(),
    }
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
    let diagnostics = code_lint::runner::lint_file(virtual_path, &payload.code, &parsed_config);
    let plain = diagnostic::format_plain(&diagnostics);
    PlaygroundResponse::Lint {
        ok: true,
        diagnostics,
        plain,
    }
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
                .map(|language| language.as_str())
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

#[cfg(test)]
mod tests {
    use super::handle_request;

    #[test]
    fn lint_action_returns_diagnostics_for_python_and_rust() {
        let python_response = handle_request(
            r#"{"action":"lint","path":"tests/test_order.py","code":"import time\n\ndef test_order():\n    time.sleep(1)\n","config":""}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&python_response).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["diagnostics"][0]["rule_name"], "sleep-in-tests");
        assert!(parsed["plain"]
            .as_str()
            .unwrap()
            .contains("[sleep-in-tests]"));
    }

    #[test]
    fn lint_action_respects_toml_config_and_reports_invalid_config() {
        let ignored_response = handle_request(
            r#"{"action":"lint","path":"tests/test_order.py","code":"import time\n\ndef test_order():\n    time.sleep(1)\n","config":"ignore = [\"sleep-in-tests\"]"}"#,
        );
        let ignored_parsed: serde_json::Value = serde_json::from_str(&ignored_response).unwrap();
        assert_eq!(ignored_parsed["ok"], true);
        assert_eq!(ignored_parsed["diagnostics"].as_array().unwrap().len(), 0);

        let invalid_response = handle_request(
            r#"{"action":"lint","path":"src/app.py","code":"x = 1\n","config":"select = [\"tesing\"]"}"#,
        );
        let invalid_parsed: serde_json::Value = serde_json::from_str(&invalid_response).unwrap();
        assert_eq!(invalid_parsed["ok"], false);
        assert!(invalid_parsed["error"]
            .as_str()
            .unwrap()
            .contains("did you mean `testing`?"));
    }

    #[test]
    fn explain_action_renders_rule_markdown_with_live_config_status() {
        let response = handle_request(
            r#"{"action":"explain","rule":"sleep-in-tests","config":"ignore = [\"testing\"]"}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed["ok"], true);
        let markdown = parsed["markdown"].as_str().unwrap();
        assert!(markdown.contains("# sleep-in-tests"));
        assert!(markdown.contains("Status: disabled by `ignore = [\"testing\"]`"));
    }
}
