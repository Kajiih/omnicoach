//! Alpha / experimental multi-level abstraction graph generator (`omnicoach graph`).
//!
//! Under active investigation and design (`docs/dev/abstraction_graph/`); the graph model,
//! grouping, and output formats may change at any time.

architecture_component!(CodeLintGraph);

mod builder;
mod index;
mod model;
mod query;
mod render;

pub use self::builder::ProjectDepth;
pub use self::query::{FindQueryOptions, SliceQueryOptions, run_find_query, run_slice_query};
pub use self::render::OutputFormat;

use self::builder::{build_file_graph, build_project_graph};
use self::render::render_graph;

use crate::code_lint::ast::{ParsedFile, collect_intra_file_references, collect_scope_facts};
use crate::code_lint::project::{
    DiskFileSource, FileSource as _, ProjectConfig, derive_project_title, discover_project_sources,
    resolve_effective_paths,
};
use crate::diagnostic::Language;
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
pub fn generate_graph(options: &GraphOptions, config: &ProjectConfig) -> anyhow::Result<String> {
    let effective_paths = resolve_effective_paths(&options.paths)?;
    let is_single_file = effective_paths.len() == 1 && DiskFileSource.is_file(&effective_paths[0]);
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
    let content = DiskFileSource.read_to_string(path)?;
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
    config: &ProjectConfig,
) -> anyhow::Result<String> {
    let discovered = discover_project_sources(paths, config)?;
    if discovered.files.is_empty() {
        anyhow::bail!("No supported source files found in the specified paths");
    }

    let title = derive_project_title(paths);
    let mut sections = Vec::new();
    for lang in [Language::Rust, Language::Python] {
        if !discovered.files.iter().any(|file| file.lang == lang) {
            continue;
        }
        let project_facts = discovered.collect_project_facts(&title, lang);
        let graph = build_project_graph(&project_facts, depth)?;
        sections.push(render_graph(&graph, format));
    }

    Ok(sections.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_file_graph_defaults_to_full_depth_and_rejects_unsupported_extension() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let single_options = GraphOptions {
            paths: vec![root.join("src/code_lint/suppression.rs")],
            depth: None,
            format: OutputFormat::AnnotatedStubs,
        };
        let unsupported_options = GraphOptions {
            paths: vec![root.join("Cargo.toml")],
            depth: None,
            format: OutputFormat::AnnotatedStubs,
        };

        let rendered = generate_graph(&single_options, &ProjectConfig::default()).unwrap();
        let unsupported = generate_graph(&unsupported_options, &ProjectConfig::default());

        assert!(rendered.contains("parse_directive_prefix"));
        assert!(unsupported.is_err());
    }

    #[test]
    fn multi_file_graph_defaults_to_module_depth() {
        let rendered =
            generate_graph(&multi_file_options(None), &ProjectConfig::default()).unwrap();
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
        let rendered = generate_graph(&options, &ProjectConfig::default()).unwrap();
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
            &ProjectConfig::default(),
        )
        .unwrap();
        assert!(
            rendered.contains("parse_directive_prefix"),
            "expected declarations at full depth in:\n{rendered}"
        );
    }

    #[test]
    fn subdirectory_graph_resolves_declared_component_without_empty_siblings() {
        let options = GraphOptions {
            paths: vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src/code_lint/project")],
            ..GraphOptions::default()
        };
        let rendered = generate_graph(&options, &ProjectConfig::default()).unwrap();
        assert!(
            rendered.contains("## Component [L0] CodeLintProject (depends_on=[])"),
            "expected declared CodeLintProject component in:\n{rendered}"
        );
        assert!(
            rendered.contains("### Module [L0] code_lint::project::config"),
            "expected L0 config module to render untruncated in:\n{rendered}"
        );
        assert!(
            rendered.contains("### Module [L1] code_lint::project::source"),
            "expected L1 source module to render untruncated in:\n{rendered}"
        );
        assert!(
            !rendered.contains("CommandLintVcs"),
            "expected unrelated components to be omitted in:\n{rendered}"
        );
    }

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
}
