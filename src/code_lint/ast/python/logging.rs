//! Python logger calls (`logging` and `loguru`): receiver and method recognition, message and
//! arguments.

use super::{AstNode, ParsedFile, static_string_text};
use crate::code_lint::ast::span_from_ruff_range;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr};
use ruff_python_ast::{Expr, ExprCall};
use ruff_text_size::{Ranged as _, TextRange};
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
    /// Byte range of the message expression (`arg 0`, or `arg 1` for `.log(level, msg, ...)`).
    pub(super) message_range: TextRange,
}

/// Returns true if `receiver` is a recognized logger variable, module, or attribute (`logger`,
/// `_logger`, `logging`, `self.logger`, `app.logger`, etc.).
fn is_logger_receiver(receiver: &Expr) -> bool {
    match receiver {
        Expr::Name(name) => LOGGER_RECEIVERS.contains(&name.id.as_str()),
        Expr::Attribute(attribute) => LOGGER_ATTRIBUTES.contains(&attribute.attr.as_str()),
        _ => false,
    }
}

/// Parses `call` as a logger call if its callee is a recognized logger receiver and method.
pub(super) fn extract_logger_call<'a>(
    call: &ExprCall,
    file: &'a ParsedFile,
) -> Option<PythonLoggerCall<'a>> {
    let Expr::Attribute(function) = call.func.as_ref() else {
        return None;
    };
    if !is_logger_receiver(&function.value) {
        return None;
    }
    let method = function.attr.as_str();
    let uses_printf = if PRINTF_LOGGER_METHODS.contains(&method) {
        true
    } else if LOGURU_ONLY_METHODS.contains(&method) {
        false
    } else {
        return None;
    };

    let mut keyword_names = HashSet::new();
    let mut has_keyword_splat = false;
    for keyword in &call.arguments.keywords {
        if let Some(arg_name) = &keyword.arg {
            keyword_names.insert(arg_name.id.to_string());
        } else {
            has_keyword_splat = true;
        }
    }

    let message_index = usize::from(method == "log");
    let message_expr = call.arguments.args.get(message_index)?;
    if matches!(message_expr, Expr::Starred(_)) {
        return None;
    }

    let func_span = span_from_ruff_range(function.range());
    Some(PythonLoggerCall {
        node: AstNode::from_span(file, span_from_ruff_range(call.range())),
        callee: file.source[func_span.start..func_span.end].to_owned(),
        message: static_string_text(message_expr, &file.source),
        has_trailing_positional_args: call.arguments.args.len() > message_index + 1,
        has_keyword_splat,
        keyword_names,
        uses_printf,
        message_range: message_expr.range(),
    })
}

/// Collects the logger calls in `file`, in source order.
#[must_use]
pub fn collect_logger_calls(file: &ParsedFile) -> Vec<PythonLoggerCall<'_>> {
    struct LoggerCallVisitor<'a> {
        file: &'a ParsedFile,
        out: Vec<PythonLoggerCall<'a>>,
    }

    impl<'a> SourceOrderVisitor<'a> for LoggerCallVisitor<'a> {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Call(call) = expr
                && let Some(logger_call) = extract_logger_call(call, self.file)
            {
                self.out.push(logger_call);
            }
            walk_expr(self, expr);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = LoggerCallVisitor {
        file,
        out: Vec::new(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}
