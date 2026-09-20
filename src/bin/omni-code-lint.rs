//! Static analysis codebase linter binary.

use omni::code_lint::lint_file;
use omni::core::Config;
use omni::diagnostic::{Diagnostic, print_diagnostics};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "omni-code-lint",
    version,
    about = "Static analysis codebase linter"
)]
struct Cli {
    /// Paths to inspect
    #[arg(default_value = ".")]
    paths: Vec<String>,

    /// Output format for diagnostics (plain, json)
    #[arg(long, default_value = "plain")]
    format: String,

    /// Run linter only on files/lines changed in VCS
    #[arg(long)]
    diff: bool,

    /// Run linter comparing against a custom revision/commit (implies --diff)
    #[arg(long)]
    diff_rev: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config =
        Config::load().map_err(|error| anyhow::anyhow!("Failed to load configuration: {error}"))?;

    let mut all_diagnostics = Vec::new();

    let enable_diff = cli.diff || cli.diff_rev.is_some();

    if enable_diff {
        for path_input in &cli.paths {
            let path = Path::new(path_input);
            if !path.exists() {
                anyhow::bail!("Path does not exist: {path_input}");
            }
        }

        let (vcs_type, resolved_rev, changes) =
            omni::diff::detect_vcs_diff(cli.diff_rev.as_deref())?;

        eprintln!("Info: Comparing against {vcs_type:?} revision '{resolved_rev}'.");

        // Resolve input target paths to absolute paths
        let target_paths: Vec<PathBuf> = cli
            .paths
            .iter()
            .map(|path_arg| {
                Path::new(path_arg)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(path_arg))
            })
            .collect();

        for (file_path, changed_lines) in &changes {
            let is_target = target_paths.iter().any(|target| {
                if target.is_file() {
                    target == file_path
                } else {
                    file_path.starts_with(target)
                }
            });

            if is_target {
                lint_single_file(
                    file_path,
                    &config,
                    &mut all_diagnostics,
                    Some(changed_lines),
                )?;
            }
        }
    } else {
        for path_input in &cli.paths {
            let path = Path::new(path_input);
            if path.is_file() {
                lint_single_file(path, &config, &mut all_diagnostics, None)?;
            } else if path.is_dir() {
                lint_directory(path, &config, &mut all_diagnostics);
            } else {
                anyhow::bail!("Path does not exist: {path_input}");
            }
        }
    }

    // Print diagnostics according to format
    if !all_diagnostics.is_empty() {
        print_diagnostics(&all_diagnostics, &cli.format)?;
        std::process::exit(1);
    }
    Ok(())
}

fn lint_single_file(
    path: &Path,
    config: &Config,
    diagnostics: &mut Vec<Diagnostic>,
    changed_lines: Option<&HashSet<usize>>,
) -> anyhow::Result<()> {
    let content = fs::read_to_string(path)
        .map_err(|error| anyhow::anyhow!("Failed to read file '{}': {}", path.display(), error))?;
    let diags = lint_file(path, &content, config);
    if let Some(changed) = changed_lines {
        let filtered = diags
            .into_iter()
            .filter(|diagnostic| changed.contains(&diagnostic.location.line));
        diagnostics.extend(filtered);
    } else {
        diagnostics.extend(diags);
    }
    Ok(())
}

fn lint_directory(dir: &Path, config: &Config, diagnostics: &mut Vec<Diagnostic>) {
    for entry in ignore::WalkBuilder::new(dir)
        .require_git(false)
        .build()
        .flatten()
    {
        let path = entry.path();
        if path.is_file() {
            if let Err(error) = lint_single_file(path, config, diagnostics, None) {
                eprintln!("Warning: {error}");
            }
        }
    }
}
