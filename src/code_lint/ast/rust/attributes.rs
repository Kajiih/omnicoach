//! Rust attributes (`#[...]` and `#![...]`): declarative metadata on items.

use ra_ap_syntax::AstNode as _;
use ra_ap_syntax::ast;

/// Extracts the terminal identifier of an attribute's path (e.g. `"test"` for `#[tokio::test]`).
pub(super) fn attribute_terminal_name(attribute: &ast::Attr) -> Option<ast::NameRef> {
    let path = attribute
        .path()
        .or_else(|| attribute.syntax().descendants().find_map(ast::Path::cast))?;
    path.segment()?.name_ref()
}
