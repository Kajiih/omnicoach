//! Alpha / experimental multi-level abstraction graph generator (`omnicoach graph`).
//!
//! Under active investigation and design (`docs/dev/abstraction_graph/01_understand.md` ..
//! `08_stage_c_agent_formats.md`); the graph model, grouping, and output formats may change at any
//! time.

architecture_component!(CodeLintGraph);

mod builder;
mod model;
mod project_facts;
mod render;

pub use self::builder::{ProjectDepth, build_file_graph, build_project_graph};
pub use self::model::{
    AbstractionGraph, DeclarationKind, DeclarationNode, EdgeReason, GraphParts, GroupKind,
    GroupNode, ModelError, NodeFinding, NodeId, NodeVisibility, OrderingEdge, ReferenceEdge,
    ReferenceKind, ReflexionStatus, SourceLocation, SummaryEdge,
};
pub use self::project_facts::{
    ComponentFact, CrossModuleRefFact, ModuleFact, ProjectFacts, collect_project_facts_from_sources,
};
pub use self::render::{ALPHA_BANNER, OutputFormat, render_graph};

use crate::code_lint::ast::{ParsedFile, collect_scope_facts};
use crate::config::Config;
use crate::diagnostic::Language;
use std::fs;
use std::path::{Path, PathBuf};

/// Options for generating a module or project abstraction graph.
#[derive(Debug, Clone)]
pub struct GraphOptions {
    /// File or directory paths to inspect (defaults to `"."` when empty).
    pub paths: Vec<PathBuf>,
    /// Hierarchy depth for project graphs.
    pub depth: ProjectDepth,
    /// Output text format (`AnnotatedStubs` or `CompactBi`).
    pub format: OutputFormat,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            depth: ProjectDepth::Full,
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
    let effective_paths = if options.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        options.paths.clone()
    };

    for path in &effective_paths {
        if !path.exists() {
            anyhow::bail!("Path does not exist: {}", path.display());
        }
    }

    if effective_paths.len() == 1
        && effective_paths[0].is_file()
        && options.depth == ProjectDepth::Full
    {
        return generate_single_file_graph(&effective_paths[0], options.format);
    }

    generate_project_graphs(&effective_paths, options.depth, options.format, config)
}

fn generate_single_file_graph(path: &Path, format: OutputFormat) -> anyhow::Result<String> {
    let Some(lang) = Language::from_path(path) else {
        anyhow::bail!("Unsupported file language: {}", path.display());
    };
    let content = fs::read_to_string(path)?;
    let parsed = ParsedFile::new(&content, lang);
    let scopes = collect_scope_facts(&parsed);
    let display_path = path.strip_prefix("./").unwrap_or(path);
    let graph = build_file_graph(display_path, &scopes)?;
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
