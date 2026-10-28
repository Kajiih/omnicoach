//! Rust type annotation unwrapping and generic path extraction (`ra_ap_syntax`).

use super::names::compact_path_text;
use crate::code_lint::ast::imports::QualifiedName;
use crate::code_lint::ast::tree::ParsedFile;
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::AstNode as _;
use ra_ap_syntax::ast::{self, HasGenericArgs as _};

/// Unqualified name of `std::option::Option`.
const OPTION_TYPE_NAME: &str = "Option";
/// Unqualified name of `std::boxed::Box`.
const BOX_TYPE_NAME: &str = "Box";
/// Unqualified name of `std::rc::Rc`.
const RC_TYPE_NAME: &str = "Rc";
/// Unqualified name of `std::sync::Arc`.
const ARC_TYPE_NAME: &str = "Arc";
/// Unqualified name of `std::borrow::Cow`.
const COW_TYPE_NAME: &str = "Cow";

/// A path type whose terminal segment has a `<...>` generic argument list (`io::Result<T>`).
pub(super) struct GenericPathType {
    /// The source span of the base path, through its terminal segment's name (`io::Result`).
    pub(super) base_span: SourceSpan,
    /// The base path resolved through the file's imports.
    pub(super) base: QualifiedName,
    /// The type arguments, excluding lifetime and associated-type arguments.
    pub(super) arguments: Vec<ast::Type>,
}

/// The element count if `type_node` (unwrapped through `Option<T>`, `&T`, `&mut T`, `Box<T>`,
/// `Rc<T>`, `Arc<T>`, `Cow<'_, T>`) is a non-unit fixed-length tuple (`(T,)`, `(A, B)`).
pub(super) fn positional_tuple_type_arity(
    type_node: &ast::Type,
    file: &ParsedFile,
) -> Option<usize> {
    let mut current = type_node.clone();
    loop {
        if let Some(payload) = option_payload(&current, file) {
            current = payload;
            continue;
        }
        let unwrapped = unwrap_rust_pointer_wrappers(current.clone(), file);
        if unwrapped.syntax().text_range() != current.syntax().text_range() {
            current = unwrapped;
            continue;
        }
        break;
    }
    match current {
        ast::Type::TupleType(tuple) => match tuple.fields().count() {
            arity @ 1.. => Some(arity),
            0 => None,
        },
        _ => None,
    }
}

/// Returns `T` if `type_node` is `Option<T>` (unqualified, `std::option::Option`, or
/// `core::option::Option`).
pub(super) fn option_payload(type_node: &ast::Type, file: &ParsedFile) -> Option<ast::Type> {
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
pub(super) fn unwrap_rust_pointer_wrappers(type_node: ast::Type, file: &ParsedFile) -> ast::Type {
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

/// Extracts the base and type arguments of an [`ast::PathType`] whose terminal segment has a
/// `<...>` generic argument list.
pub(super) fn extract_path_type_generics(
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
