//! Python logger calls (`logging` and `loguru`): receiver and method recognition, message and
//! arguments.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use super::{AstNode, ParsedFile, RawNode, static_string_text};
use std::collections::HashSet;

/// Bare variable/module names recognized as logger receivers (`logger.info`, `logging.error`, `_logger.info`).
const LOGGER_RECEIVERS: &[&str] = &["logging", "logger", "log", "_logger", "_log"];

/// Attribute names recognized on compound logger receivers (`self.logger.info`, `app.logger.warn`).
const LOGGER_ATTRIBUTES: &[&str] = &["logger", "log", "_logger", "_log"];

/// Standard `logging` and `loguru` methods that accept `%`-formatted positional arguments in stdlib `logging`.
const PRINTF_LOGGER_METHODS: &[&str] = &[
    "debug",
    "info",
    "warning",
    "warn",
    "error",
    "critical",
    "fatal",
    "exception",
    "log",
];

/// Additional `loguru`-only methods that format with `{}` rather than `%`.
const LOGURU_ONLY_METHODS: &[&str] = &["trace", "success"];

/// A call to a recognized logger method (`logger.info(...)`, `self.log.error(...)`).
pub struct PythonLoggerCall<'a> {
    /// The `call` node.
    pub node: AstNode<'a>,
    /// Source text of the invoked method (`logger.info`, `app.logger.error`).
    pub callee: String,
    /// Static text of the message when it is a plain string literal or an implicit
    /// concatenation of them.
    pub message: Option<String>,
    /// True if at least one positional or `*args` argument follows the message.
    pub has_trailing_positional_args: bool,
    /// True if the call contains a `**kwargs` dictionary splat.
    pub has_keyword_splat: bool,
    /// Explicit keyword argument names passed to the call.
    pub keyword_names: HashSet<String>,
    /// Whether this method supports stdlib `logging` `%`-formatting (`false` for `trace`/`success`).
    pub(super) uses_printf: bool,
    /// The message expression node (`arg 0`, or `arg 1` for `.log(level, msg, ...)`).
    pub(super) message_node: RawNode<'a>,
}

/// Returns true if `receiver` is a recognized logger variable, module, or attribute (`logger`,
/// `_logger`, `logging`, `self.logger`, `app.logger`, etc.).
fn is_logger_receiver(receiver: &RawNode<'_>) -> bool {
    match receiver.kind().as_ref() {
        "identifier" => LOGGER_RECEIVERS.contains(&receiver.text().as_ref()),
        "attribute" => receiver
            .field("attribute")
            .is_some_and(|attribute| LOGGER_ATTRIBUTES.contains(&attribute.text().as_ref())),
        _ => false,
    }
}

/// Parses `call_node` as a logger call if its callee is a recognized logger receiver and method.
pub(super) fn extract_logger_call<'a>(call_node: &RawNode<'a>) -> Option<PythonLoggerCall<'a>> {
    if call_node.kind() != "call" {
        return None;
    }
    let function = call_node.field("function")?;
    if function.kind() != "attribute" {
        return None;
    }
    let receiver = function.field("object")?;
    if !is_logger_receiver(&receiver) {
        return None;
    }
    let method_node = function.field("attribute")?;
    let method = method_node.text();
    let uses_printf = if PRINTF_LOGGER_METHODS.contains(&method.as_ref()) {
        true
    } else if LOGURU_ONLY_METHODS.contains(&method.as_ref()) {
        false
    } else {
        return None;
    };

    let arguments = call_node.field("arguments")?;
    let mut positional_or_splat = Vec::new();
    let mut keyword_names = HashSet::new();
    let mut has_keyword_splat = false;

    for child in arguments
        .children()
        .filter(|child| child.is_named() && !child.is_extra())
    {
        match child.kind().as_ref() {
            "dictionary_splat" => has_keyword_splat = true,
            "keyword_argument" => {
                if let Some(name_node) = child.field("name") {
                    keyword_names.insert(name_node.text().into_owned());
                }
            }
            _ => positional_or_splat.push(child),
        }
    }

    let message_index = usize::from(method == "log");
    let message_node = positional_or_splat.get(message_index)?.clone();
    if message_node.kind() == "list_splat" {
        return None;
    }

    Some(PythonLoggerCall {
        node: AstNode::from_raw(call_node.clone()),
        callee: function.text().into_owned(),
        message: static_string_text(&message_node),
        has_trailing_positional_args: positional_or_splat.len() > message_index + 1,
        has_keyword_splat,
        keyword_names,
        uses_printf,
        message_node,
    })
}

/// Collects the logger calls in `file`, in source order.
#[must_use]
pub fn collect_logger_calls(file: &ParsedFile) -> Vec<PythonLoggerCall<'_>> {
    file.grep
        .root()
        .dfs()
        .filter_map(|node| extract_logger_call(&node))
        .collect()
}
