//! Shared Python logger call recognition (`logging` and `loguru`) and `unmatched-logger-placeholder` collector.

// omni:disable-file [repeated-literal] -- Tree-sitter node kinds and field names (see ROADMAP)

use super::{
    AstNode, ParsedFile, RawNode, extract_logger_message_literal, first_unmatched_named_placeholder,
};
use std::collections::HashSet;

/// A Python logger call with an unmatched named PEP 3101 placeholder and positional arguments.
#[derive(Clone)]
pub struct UnmatchedLoggerPlaceholder<'a> {
    /// The full `call` AST node (`logger.info("Order {order_id}", order_id)`).
    pub call_node: AstNode<'a>,
    /// Source text of the invoked logger method (`logger.info`, `self.logger.error`).
    pub callee: String,
    /// The first unmatched named placeholder root identifier (`order_id`).
    pub placeholder: String,
}

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

/// Parsed metadata for a recognized logger method call.
pub(super) struct LoggerCallInfo<'a> {
    /// Full callee text (`logger.info`, `app.logger.error`).
    pub callee: String,
    /// Whether this method supports stdlib `logging` `%`-formatting (`false` for `trace`/`success`).
    pub uses_printf: bool,
    /// The message expression node (`arg 0`, or `arg 1` for `.log(level, msg, ...)`).
    pub message_node: RawNode<'a>,
    /// True if at least one positional or `*args` argument follows `message_node`.
    pub has_trailing_positional_args: bool,
    /// True if the call contains a `**kwargs` dictionary splat.
    pub has_keyword_splat: bool,
    /// Explicit keyword argument names passed to the call.
    pub keyword_names: HashSet<String>,
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
pub(super) fn extract_logger_call<'a>(call_node: &RawNode<'a>) -> Option<LoggerCallInfo<'a>> {
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

    Some(LoggerCallInfo {
        callee: function.text().into_owned(),
        uses_printf,
        message_node,
        has_trailing_positional_args: positional_or_splat.len() > message_index + 1,
        has_keyword_splat,
        keyword_names,
    })
}

/// Inspects a single `call` node and returns an [`UnmatchedLoggerPlaceholder`] if it is a
/// logger call passing positional format arguments to a message with an unmatched named
/// placeholder.
fn check_logger_call_node<'a>(call_node: &RawNode<'a>) -> Option<UnmatchedLoggerPlaceholder<'a>> {
    let info = extract_logger_call(call_node)?;
    if info.has_keyword_splat || !info.has_trailing_positional_args {
        return None;
    }
    let message = extract_logger_message_literal(&info.message_node)?;
    let placeholder = first_unmatched_named_placeholder(&message, &info.keyword_names)?;

    Some(UnmatchedLoggerPlaceholder {
        call_node: AstNode::from_raw(call_node.clone()),
        callee: info.callee,
        placeholder,
    })
}

/// Collects logger calls in `file` that pass positional format arguments to a message literal
/// containing an unmatched named PEP 3101 placeholder.
#[must_use]
pub fn collect_unmatched_logger_placeholders(
    file: &ParsedFile,
) -> Vec<UnmatchedLoggerPlaceholder<'_>> {
    file.grep
        .root()
        .dfs()
        .filter(|node| node.kind() == "call")
        .filter_map(|node| check_logger_call_node(&node))
        .collect()
}
