//! Function definitions, return shapes, and nullable return payloads for Rust (`ra_ap_syntax`).

use super::annotations::{
    GenericPathType, extract_path_type_generics, option_payload, positional_tuple_type_arity,
    unwrap_rust_pointer_wrappers,
};
use super::tree::cast_at_span;
use crate::code_lint::ast::imports::QualifiedName;
use crate::code_lint::ast::returns::{FunctionReturn, ReturnShape};
use crate::code_lint::ast::tree::{AstNode, ParsedFile, span_from_rowan_range};
use ra_ap_syntax::AstNode as _;
use ra_ap_syntax::ast::{self, HasName as _};

/// Unqualified name of `std::task::Poll`.
const POLL_TYPE_NAME: &str = "Poll";
/// The Rust boolean primitive.
const BOOL_TYPE_NAME: &str = "bool";

/// The payload `T` of an optional return type, as classified by [`nullable_return_payload`].
pub enum NullableReturnPayload {
    /// A slice or unsized array type (`[T]`), with its source text.
    Slice(String),
    /// A generic path type (`std::vec::Vec<T>`).
    Generic {
        /// Source text of the base path (`std::vec::Vec`).
        base: String,
        /// The base path with whitespace and any leading `::` removed.
        path: String,
        /// The last segment of the base path (`Vec`).
        terminal: String,
    },
}

/// See [`crate::code_lint::ast::collect_function_returns`]: trait items, trait implementations and
/// inline tests are skipped.
pub fn collect_function_returns(file: &ParsedFile) -> Vec<FunctionReturn<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for function in parsed
        .tree()
        .syntax()
        .descendants()
        .filter_map(ast::Fn::cast)
    {
        let node = AstNode::from_span(file, span_from_rowan_range(function.syntax().text_range()));
        let is_trait_or_trait_impl = is_trait_or_trait_impl_function(&function);
        if is_trait_or_trait_impl || node.is_in_rust_inline_test() {
            continue;
        }
        let Some(name_node) = function.name() else {
            continue;
        };
        let return_type = function.ret_type().and_then(|ret| ret.ty());
        let declared_type = return_type.as_ref().map(|type_node| {
            AstNode::from_span(file, span_from_rowan_range(type_node.syntax().text_range()))
        });
        out.push(FunctionReturn {
            name: name_node.text().trim().to_owned(),
            name_node: AstNode::from_span(
                file,
                span_from_rowan_range(name_node.syntax().text_range()),
            ),
            declared_type,
            shape: return_shape(return_type, file),
        });
    }
    out
}

/// Classifies the payload `T` of `return_type` if it is `Option<T>`.
///
/// Outer `Result<T, ...>` (and aliases ending in `Result`) and `Poll<T>` envelopes are unwrapped
/// first, then transparent borrow and smart-pointer wrappers (`&T`, `&mut T`, `Box<T>`, `Rc<T>`,
/// `Arc<T>`, `Cow<'_, T>`) around `T`. Returns `None` if there is no `Option` or `T` is neither a
/// slice nor a generic path type.
#[must_use]
pub fn nullable_return_payload(return_type: &AstNode<'_>) -> Option<NullableReturnPayload> {
    let file = return_type.file;
    let source = &file.source;
    let return_type = cast_at_span::<ast::Type>(return_type)?;
    let payload = option_payload(&unwrap_rust_return_envelope(return_type, file), file)?;
    let inner = unwrap_rust_pointer_wrappers(payload, file);
    match &inner {
        ast::Type::SliceType(_) => {
            let span = span_from_rowan_range(inner.syntax().text_range());
            Some(NullableReturnPayload::Slice(
                source[span.start..span.end].trim().to_owned(),
            ))
        }
        ast::Type::ArrayType(array_type) if array_type.const_arg().is_none() => {
            let span = span_from_rowan_range(inner.syntax().text_range());
            Some(NullableReturnPayload::Slice(
                source[span.start..span.end].trim().to_owned(),
            ))
        }
        ast::Type::PathType(path_type) => {
            let GenericPathType {
                base_span,
                base: QualifiedName { path, terminal },
                ..
            } = extract_path_type_generics(path_type, file)?;
            Some(NullableReturnPayload::Generic {
                base: source[base_span.start..base_span.end].trim().to_owned(),
                path,
                terminal,
            })
        }
        _ => None,
    }
}

/// Returns true if `function` is declared inside a `trait` block or an `impl Trait for Type` block.
pub(super) fn is_trait_or_trait_impl_function(function: &ast::Fn) -> bool {
    let Some(assoc_items) = function.syntax().parent() else {
        return false;
    };
    if !ast::AssocItemList::can_cast(assoc_items.kind()) {
        return false;
    }
    let Some(enclosing_item) = assoc_items.parent() else {
        return false;
    };
    ast::Trait::can_cast(enclosing_item.kind())
        || ast::Impl::cast(enclosing_item).is_some_and(|impl_item| impl_item.trait_().is_some())
}

/// What the function returns, read from its return type; see [`ReturnShape`].
fn return_shape(return_type: Option<ast::Type>, file: &ParsedFile) -> ReturnShape {
    let Some(type_node) = return_type else {
        return ReturnShape::Unit;
    };
    let success = unwrap_rust_return_envelope(type_node, file);
    match &success {
        ast::Type::TupleType(tuple) if tuple.fields().next().is_none() => {
            return ReturnShape::Unit;
        }
        ast::Type::PathType(path_type) if path_type.syntax().text() == BOOL_TYPE_NAME => {
            return ReturnShape::Boolean;
        }
        _ => {}
    }
    positional_tuple_type_arity(&success, file).map_or(ReturnShape::Other, |arity| {
        ReturnShape::PositionalTuple { arity }
    })
}

/// Unwraps outer `Result<T, ...>` (or an alias whose last segment ends in `Result`, such as
/// `io::Result<T>` or `CargoResult<T>`) and `Poll<T>` return type envelopes from `type_node`.
fn unwrap_rust_return_envelope(type_node: ast::Type, file: &ParsedFile) -> ast::Type {
    let mut current = type_node;
    while let ast::Type::PathType(ref path_type) = current {
        let Some(GenericPathType {
            base:
                QualifiedName {
                    path: base_path,
                    terminal: base_terminal,
                },
            arguments: type_args,
            ..
        }) = extract_path_type_generics(path_type, file)
        else {
            break;
        };
        let is_result_or_poll = base_terminal.ends_with("Result")
            || (base_terminal == POLL_TYPE_NAME
                && matches!(
                    base_path.as_str(),
                    POLL_TYPE_NAME | "task::Poll" | "std::task::Poll" | "core::task::Poll"
                ));
        if is_result_or_poll && let Some(first_arg) = type_args.into_iter().next() {
            current = first_arg;
        } else {
            break;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_function_returns_rust() {
        let source = indoc::indoc! {r"
            fn free_fn() -> Option<u32> { None }
            trait T {
                fn trait_fn(&self) {}
            }
            impl T for S {
                fn trait_fn(&self) {}
            }
            impl S {
                fn inherent_method(&self) -> Vec<u8> { Vec::new() }
            }
        "};
        let file = ParsedFile::rust(source);
        let functions = collect_function_returns(&file);
        let actual: Vec<_> = functions
            .iter()
            .map(|func| {
                (
                    func.name.as_str(),
                    func.declared_type
                        .as_ref()
                        .map(|ret_type| ret_type.text().to_string()),
                )
            })
            .collect();
        assert_eq!(
            actual,
            &[
                ("free_fn", Some("Option<u32>".to_owned())),
                ("inherent_method", Some("Vec<u8>".to_owned())),
            ]
        );
    }

    #[rstest::rstest]
    #[case::bare_option("fn f() -> Option<Vec<u8>> { None }", "Vec<u8>", "Vec<u8>")]
    #[case::result_option(
        "fn f() -> Result<Option<String>, ()> { Ok(None) }",
        "String",
        "String"
    )]
    #[case::borrowed_slice("fn f() -> Option<&'a [u8]> { None }", "&'a [u8]", "[u8]")]
    #[case::boxed_slice("fn f() -> Option<Box<[u8]>> { None }", "Box<[u8]>", "[u8]")]
    fn test_return_type_unwrapping_rust(
        #[case] source: &str,
        #[case] expected_payload: &str,
        #[case] expected_unwrapped: &str,
    ) {
        let file = ParsedFile::rust(source);
        let functions = collect_function_returns(&file);
        let return_type = cast_at_span::<ast::Type>(functions[0].declared_type.as_ref().unwrap())
            .expect("return type");
        let unwrapped_envelope = unwrap_rust_return_envelope(return_type, &file);
        let payload = option_payload(&unwrapped_envelope, &file).expect("option payload");
        let payload_text = payload.syntax().text().to_string();
        let inner = unwrap_rust_pointer_wrappers(payload, &file);
        assert_eq!(payload_text, expected_payload);
        assert_eq!(inner.syntax().text().to_string(), expected_unwrapped);
    }
}
