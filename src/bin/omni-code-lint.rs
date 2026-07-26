//! Static analysis codebase linter binary.

use omni::code_lint::lint_file;
use omni::core::Config;
use omni::diagnostic::{print_diagnostics, Diagnostic};
use std::fs;
use std::path::Path;

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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load config (or default config)
    let config = match Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("Error: Failed to load configuration: {err}");
            std::process::exit(2);
        }
    };

    let mut all_diagnostics = Vec::new();

    for path_str in &cli.paths {
        let path = Path::new(path_str);
        if path.is_file() {
            lint_single_file(path, &config, &mut all_diagnostics);
        } else if path.is_dir() {
            lint_directory(path, &config, &mut all_diagnostics);
        }
    }

    // Print diagnostics according to format
    if !all_diagnostics.is_empty() {
        print_diagnostics(&all_diagnostics, &cli.format)?;
        std::process::exit(1);
    }
    Ok(())
}

fn lint_single_file(path: &Path, config: &Config, diagnostics: &mut Vec<Diagnostic>) {
    if let Ok(content) = fs::read_to_string(path) {
        let diags = lint_file(path, &content, config);
        diagnostics.extend(diags);
    }
}

fn lint_directory(dir: &Path, config: &Config, diagnostics: &mut Vec<Diagnostic>) {
    for entry in ignore::WalkBuilder::new(dir).build().flatten() {
        let path = entry.path();
        if path.is_file() {
            lint_single_file(path, config, diagnostics);
        }
    }
}
