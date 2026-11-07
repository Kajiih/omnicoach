//! Alpha / experimental multi-level abstraction graph generator (`omnicoach graph`).
//!
//! Under active investigation and design (`docs/dev/abstraction_graph/`); the graph model,
//! grouping, and output formats may change at any time.

architecture_component!(CodeLintGraph);

mod builder;
mod facts;
mod model;
mod query;
mod render;

pub use self::builder::ProjectDepth;
pub use self::query::{FindQueryOptions, SliceQueryOptions, run_find_query, run_slice_query};
pub use self::render::OutputFormat;

use self::builder::{build_file_graph, build_project_graph};
use self::render::render_graph;

use self::facts::{
    collect_project_facts_with_config, collect_project_source_files, derive_project_title,
    resolve_effective_paths,
};
use crate::code_lint::ast::{ParsedFile, collect_intra_file_references, collect_scope_facts};
use crate::config::Config;
use crate::diagnostic::Language;
use std::fs;
use std::path::{Path, PathBuf};

/// Options for generating a module or project abstraction graph.
#[derive(Debug, Clone)]
pub struct GraphOptions {
    /// File or directory paths to inspect (defaults to `"."` when empty).
    pub paths: Vec<PathBuf>,
    /// Hierarchy depth, or `None` to pick it from the target: `Full` for a single file and
    /// `ModuleOnly` for a directory or several files.
    pub depth: Option<ProjectDepth>,
    /// Output text format (`AnnotatedStubs` or `CompactBi`).
    pub format: OutputFormat,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            depth: None,
            format: OutputFormat::AnnotatedStubs,
        }
    }
}

/// Generates a rendered abstraction graph for `options`.
///
/// # Errors
///
/// Returns an error if any target path does not exist, if no supported source files are found, or
/// if graph validation fails.
pub fn generate_graph(options: &GraphOptions, config: &Config) -> anyhow::Result<String> {
    let effective_paths = resolve_effective_paths(&options.paths)?;
    let is_single_file = effective_paths.len() == 1 && effective_paths[0].is_file();
    let depth = options.depth.unwrap_or(if is_single_file {
        ProjectDepth::Full
    } else {
        ProjectDepth::ModuleOnly
    });

    if is_single_file && depth == ProjectDepth::Full {
        return generate_single_file_graph(&effective_paths[0], options.format);
    }

    generate_project_graphs(&effective_paths, depth, options.format, config)
}

fn generate_single_file_graph(path: &Path, format: OutputFormat) -> anyhow::Result<String> {
    let Some(lang) = Language::from_path(path) else {
        anyhow::bail!("Unsupported file language: {}", path.display());
    };
    let content = fs::read_to_string(path)?;
    let parsed = ParsedFile::new(&content, lang);
    let scopes = collect_scope_facts(&parsed);
    let intra_refs = collect_intra_file_references(&parsed, &scopes);
    let display_path = path.strip_prefix("./").unwrap_or(path);
    let graph = build_file_graph(display_path, &scopes, &intra_refs)?;
    Ok(render_graph(&graph, format))
}

fn generate_project_graphs(
    paths: &[PathBuf],
    depth: ProjectDepth,
    format: OutputFormat,
    config: &Config,
) -> anyhow::Result<String> {
    let files = collect_project_source_files(paths, config)?;
    if files.is_empty() {
        anyhow::bail!("No supported source files found in the specified paths");
    }

    let title = derive_project_title(paths);
    let mut sections = Vec::new();
    for lang in [Language::Rust, Language::Python] {
        let lang_files: Vec<(&str, &str)> = files
            .iter()
            .filter(|file| file.lang == lang)
            .map(|file| (file.relative_path.as_str(), file.source.as_str()))
            .collect();
        if lang_files.is_empty() {
            continue;
        }
        let project_facts = collect_project_facts_with_config(
            &title,
            lang,
            &lang_files,
            config.architecture.as_ref(),
        );
        let graph = build_project_graph(&project_facts, depth)?;
        sections.push(render_graph(&graph, format));
    }

    Ok(sections.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn multi_file_options(depth: Option<ProjectDepth>) -> GraphOptions {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        GraphOptions {
            paths: vec![
                root.join("src/config.rs"),
                root.join("src/rule_selection.rs"),
                root.join("src/code_lint/suppression.rs"),
            ],
            depth,
            format: OutputFormat::AnnotatedStubs,
        }
    }

    #[test]
    fn multi_file_graph_defaults_to_module_depth() {
        let rendered = generate_graph(&multi_file_options(None), &Config::default()).unwrap();
        assert!(
            rendered.contains("suppression"),
            "expected the suppression module in:\n{rendered}"
        );
        assert!(
            !rendered.contains("parse_directive_prefix"),
            "expected no declarations at module depth in:\n{rendered}"
        );
        assert!(
            !rendered.contains("Module edges omitted"),
            "expected module edges for three modules in:\n{rendered}"
        );
    }

    #[test]
    fn module_depth_graph_of_many_modules_omits_module_edges_with_notice() {
        let options = GraphOptions {
            paths: vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
            ..GraphOptions::default()
        };
        let rendered = generate_graph(&options, &Config::default()).unwrap();
        assert!(
            rendered.contains("// Module edges omitted for "),
            "expected an omission notice in:\n{rendered}"
        );
        assert!(
            !rendered.contains("### Module [L0] config\n// "),
            "expected no module edges in:\n{rendered}"
        );
    }

    #[test]
    fn multi_file_graph_honors_explicit_full_depth() {
        let rendered = generate_graph(
            &multi_file_options(Some(ProjectDepth::Full)),
            &Config::default(),
        )
        .unwrap();
        assert!(
            rendered.contains("parse_directive_prefix"),
            "expected declarations at full depth in:\n{rendered}"
        );
    }
}
