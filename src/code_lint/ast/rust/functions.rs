//! Function definitions, return shapes, and nullable return payloads for Rust (`ra_ap_syntax`).

use super::{cast_at_span, compact_path_text};
use crate::code_lint::ast::{
    AstNode, FunctionReturn, ParsedFile, QualifiedName, ReturnShape, span_from_rowan_range,
};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::AstNode as _;
use ra_ap_syntax::ast::{self, HasGenericArgs as _, HasName as _};

/// Unqualified name of `std::option::Option`.
const OPTION_TYPE_NAME: &str = "Option";
/// Unqualified name of `std::task::Poll`.
const POLL_TYPE_NAME: &str = "Poll";
/// Unqualified name of `std::boxed::Box`.
const BOX_TYPE_NAME: &str = "Box";
/// Unqualified name of `std::rc::Rc`.
const RC_TYPE_NAME: &str = "Rc";
/// Unqualified name of `std::sync::Arc`.
const ARC_TYPE_NAME: &str = "Arc";
/// Unqualified name of `std::borrow::Cow`.
const COW_TYPE_NAME: &str = "Cow";
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
pub(in crate::code_lint::ast) fn collect_function_returns(
    file: &ParsedFile,
) -> Vec<FunctionReturn<'_>> {
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

/// Returns true if `function` is declared inside a `trait` block or an `impl Trait for Type` block.
fn is_trait_or_trait_impl_function(function: &ast::Fn) -> bool {
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
    let value = option_payload(&success, file).unwrap_or(success);
    match unwrap_rust_pointer_wrappers(value, file) {
        ast::Type::TupleType(tuple) => match tuple.fields().count() {
            arity @ 2.. => ReturnShape::PositionalTuple { arity },
            _ => ReturnShape::Other,
        },
        _ => ReturnShape::Other,
    }
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

/// Returns `T` if `type_node` is `Option<T>` (unqualified, `std::option::Option`, or
/// `core::option::Option`).
fn option_payload(type_node: &ast::Type, file: &ParsedFile) -> Option<ast::Type> {
    let ast::Type::PathType(path_type) = type_node else {
        return None;
    };
    let GenericPathType {
        base:
            QualifiedName {
                path: option_path,
                terminal: option_terminal,
            },
        arguments: option_args,
        ..
    } = extract_path_type_generics(path_type, file)?;
    if option_terminal != OPTION_TYPE_NAME
        || !matches!(
            option_path.as_str(),
            OPTION_TYPE_NAME | "option::Option" | "std::option::Option" | "core::option::Option"
        )
        || option_args.len() != 1
    {
        return None;
    }
    option_args.into_iter().next()
}

/// Unwraps transparent borrow and smart-pointer wrappers (`&T`, `&mut T`, `Box<T>`, `Rc<T>`,
/// `Arc<T>`, `Cow<'_, T>`) around a Rust type node.
fn unwrap_rust_pointer_wrappers(type_node: ast::Type, file: &ParsedFile) -> ast::Type {
    let mut current = type_node;
    loop {
        match &current {
            ast::Type::RefType(ref_type) => {
                if let Some(inner) = ref_type.ty() {
                    current = inner;
                    continue;
                }
                break;
            }
            ast::Type::PathType(path_type) => {
                if let Some(GenericPathType {
                    base:
                        QualifiedName {
                            path: base_path,
                            terminal: base_terminal,
                        },
                    arguments: type_args,
                    ..
                }) = extract_path_type_generics(path_type, file)
                {
                    let is_pointer_wrapper = match base_terminal.as_str() {
                        BOX_TYPE_NAME => matches!(
                            base_path.as_str(),
                            BOX_TYPE_NAME | "boxed::Box" | "std::boxed::Box" | "alloc::boxed::Box"
                        ),
                        RC_TYPE_NAME => matches!(
                            base_path.as_str(),
                            RC_TYPE_NAME | "rc::Rc" | "std::rc::Rc" | "alloc::rc::Rc"
                        ),
                        ARC_TYPE_NAME => matches!(
                            base_path.as_str(),
                            ARC_TYPE_NAME | "sync::Arc" | "std::sync::Arc" | "alloc::sync::Arc"
                        ),
                        COW_TYPE_NAME => matches!(
                            base_path.as_str(),
                            COW_TYPE_NAME
                                | "borrow::Cow"
                                | "std::borrow::Cow"
                                | "alloc::borrow::Cow"
                        ),
                        _ => false,
                    };
                    if is_pointer_wrapper
                        && type_args.len() == 1
                        && let Some(first_arg) = type_args.into_iter().next()
                    {
                        current = first_arg;
                        continue;
                    }
                }
                break;
            }
            _ => break,
        }
    }
    current
}

/// A path type whose terminal segment has a `<...>` generic argument list (`io::Result<T>`).
struct GenericPathType {
    /// The source span of the base path, through its terminal segment's name (`io::Result`).
    base_span: SourceSpan,
    /// The base path resolved through the file's imports.
    base: QualifiedName,
    /// The type arguments, excluding lifetime and associated-type arguments.
    arguments: Vec<ast::Type>,
}

/// Extracts the base and type arguments of an [`ast::PathType`] whose terminal segment has a
/// `<...>` generic argument list.
fn extract_path_type_generics(
    path_type: &ast::PathType,
    file: &ParsedFile,
) -> Option<GenericPathType> {
    let path = path_type.path()?;
    let segment = path.segment()?;
    let name_ref = segment.name_ref()?;
    let generic_args = segment.generic_arg_list()?;
    let base_start: usize = path.syntax().text_range().start().into();
    let base_end: usize = name_ref.syntax().text_range().end().into();
    let base_span = SourceSpan::new(base_start, base_end);
    let normalized = normalize_type_path_text(&file.source[base_start..base_end])?;
    let base_resolved = normalized.resolve(file)?;
    let type_arguments = generic_args
        .generic_args()
        .filter_map(|generic_arg| match generic_arg {
            ast::GenericArg::TypeArg(type_arg) => type_arg.ty(),
            _ => None,
        })
        .collect();
    Some(GenericPathType {
        base_span,
        base: base_resolved,
        arguments: type_arguments,
    })
}

/// Normalizes a raw type path string by stripping whitespace and any leading `::`.
fn normalize_type_path_text(raw_text: &str) -> Option<QualifiedName> {
    let compact = compact_path_text(raw_text);
    if compact.is_empty() || compact.contains('<') || compact.contains('[') || compact.contains('(')
    {
        return None;
    }
    let normalized = compact
        .strip_prefix("::")
        .unwrap_or(compact.as_str())
        .to_owned();
    Some(QualifiedName::from_path(normalized, "::"))
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
