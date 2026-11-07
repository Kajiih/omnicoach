//! Alpha / experimental multi-level abstraction graph generator (`omnicoach graph`).
//!
//! Under active investigation and design (`docs/dev/abstraction_graph/`); the graph model,
//! grouping, and output formats may change at any time.

architecture_component!(CodeLintGraph);

mod builder;
mod model;
mod project_facts;
mod query;
mod render;

pub use self::builder::ProjectDepth;
pub use self::query::{FindQueryOptions, SliceQueryOptions, run_find_query, run_slice_query};
pub use self::render::OutputFormat;

use self::builder::{build_file_graph, build_project_graph};
use self::project_facts::collect_project_facts_from_sources;
use self::render::render_graph;

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
        let project_facts = collect_project_facts_from_sources(&title, lang, &lang_files);
        let graph = build_project_graph(&project_facts, depth)?;
        sections.push(render_graph(&graph, format));
    }

    Ok(sections.join("\n"))
}

struct CollectedSourceFile {
    relative_path: String,
    lang: Language,
    source: String,
}

fn collect_project_source_files(
    paths: &[PathBuf],
    config: &Config,
) -> anyhow::Result<Vec<CollectedSourceFile>> {
    let mut collected = Vec::new();
    for path in paths {
        if path.is_file() {
            let Some(lang) = Language::from_path(path) else {
                continue;
            };
            let source = fs::read_to_string(path)?;
            collected.push(CollectedSourceFile {
                relative_path: normalize_relative_path(path, None),
                lang,
                source,
            });
        } else if path.is_dir() {
            collect_directory_source_files(path, config, &mut collected);
        }
    }
    collected.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    collected.dedup_by(|left, right| left.relative_path == right.relative_path);
    Ok(collected)
}

const SRC_DIR_NAME: &str = "src";

fn collect_directory_source_files(dir: &Path, config: &Config, out: &mut Vec<CollectedSourceFile>) {
    let walk_root = if dir.join("Cargo.toml").is_file() && dir.join(SRC_DIR_NAME).is_dir() {
        dir.join(SRC_DIR_NAME)
    } else {
        dir.to_path_buf()
    };

    for entry in ignore::WalkBuilder::new(&walk_root)
        .require_git(false)
        .build()
        .flatten()
    {
        let entry_path = entry.path();
        if !entry_path.is_file() || config.is_test_path(entry_path) {
            continue;
        }
        let Some(lang) = Language::from_path(entry_path) else {
            continue;
        };
        let Ok(source) = fs::read_to_string(entry_path) else {
            continue;
        };
        out.push(CollectedSourceFile {
            relative_path: normalize_relative_path(entry_path, Some(dir)),
            lang,
            source,
        });
    }
}

fn normalize_relative_path(path: &Path, base_dir: Option<&Path>) -> String {
    let clean_path = path.strip_prefix("./").unwrap_or(path);
    let relative = base_dir.map_or(clean_path, |base| {
        let clean_base = base.strip_prefix("./").unwrap_or(base);
        if clean_base == Path::new(".") || clean_base.starts_with(SRC_DIR_NAME) {
            clean_path
        } else {
            clean_path.strip_prefix(clean_base).unwrap_or(clean_path)
        }
    });
    relative.to_string_lossy().replace('\\', "/")
}

fn resolve_effective_paths(paths: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let effective = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    for path in &effective {
        if !path.exists() {
            anyhow::bail!("Path does not exist: {}", path.display());
        }
    }
    Ok(effective)
}

fn derive_project_title(paths: &[PathBuf]) -> String {
    if let [only] = paths {
        let clean = only.strip_prefix("./").unwrap_or(only);
        if clean != Path::new(".")
            && let Some(name) = clean.file_name().and_then(|os| os.to_str())
            && !name.is_empty()
        {
            return name.to_owned();
        }
        if let Ok(cwd) = std::env::current_dir()
            && let Some(name) = cwd.file_name().and_then(|os| os.to_str())
            && !name.is_empty()
        {
            return name.to_owned();
        }
    }
    "project".to_owned()
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
