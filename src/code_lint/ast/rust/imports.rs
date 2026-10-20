//! Rust root-level `use` and item definition map extraction (`ra_ap_syntax`).

use super::names::compact_path_text;
use crate::code_lint::ast::imports::ImportMap;
use crate::code_lint::ast::tree::ParsedFile;
use ra_ap_syntax::AstNode as _;
use ra_ap_syntax::ast::{self, HasModuleItem as _, HasName as _};

/// Collects the root-level `use` trees and item definitions of `file` (see
/// [`crate::code_lint::ast::resolve_name`]).
#[must_use]
pub(in crate::code_lint::ast) fn collect_import_map(file: &ParsedFile) -> ImportMap {
    let mut map = ImportMap::default();
    let Some(parsed) = file.rs_parsed() else {
        return map;
    };
    for item in parsed.tree().items() {
        match item {
            ast::Item::Use(use_item) => {
                if let Some(use_tree) = use_item.use_tree() {
                    collect_use_tree(&use_tree, "", &mut map);
                }
            }
            item => {
                if let Some(name) = rust_item_name(&item) {
                    map.locals.insert(name);
                }
            }
        }
    }
    map
}

/// Walks a Rust `UseTree` and records each bound name in `map`.
fn collect_use_tree(use_tree: &ast::UseTree, prefix: &str, map: &mut ImportMap) {
    let path_text = use_tree
        .path()
        .map(|path| compact_path_text(&path.syntax().text().to_string()));
    let full_path = match path_text.as_deref() {
        None => prefix.to_owned(),
        Some("self") if !prefix.is_empty() => prefix.to_owned(),
        Some(path) if prefix.is_empty() => path.trim_start_matches("::").to_owned(),
        Some(path) => format!("{prefix}::{path}"),
    };
    if let Some(sub_trees) = use_tree.use_tree_list() {
        for child_tree in sub_trees.use_trees() {
            collect_use_tree(&child_tree, &full_path, map);
        }
        return;
    }
    if use_tree.star_token().is_some() || full_path.is_empty() {
        return;
    }
    let bound = match use_tree.rename() {
        Some(rename) => match rename.name() {
            Some(name) => name.text().to_string(),
            None => return,
        },
        None => full_path
            .rsplit("::")
            .next()
            .unwrap_or(&full_path)
            .to_owned(),
    };
    map.imports.insert(bound, full_path);
}

/// The name a root-level Rust item defines, if it is a named definition.
fn rust_item_name(item: &ast::Item) -> Option<String> {
    let name = match item {
        ast::Item::Fn(item) => item.name(),
        ast::Item::Struct(item) => item.name(),
        ast::Item::Enum(item) => item.name(),
        ast::Item::Union(item) => item.name(),
        ast::Item::Trait(item) => item.name(),
        ast::Item::TypeAlias(item) => item.name(),
        ast::Item::Const(item) => item.name(),
        ast::Item::Static(item) => item.name(),
        ast::Item::Module(item) => item.name(),
        ast::Item::MacroRules(item) => item.name(),
        _ => None,
    }?;
    Some(name.text().to_string())
}
