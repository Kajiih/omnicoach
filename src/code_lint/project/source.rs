//! Filesystem abstraction for disk, hermetic in-memory tests, and unsaved hook buffer overlays.

use super::config::{is_rooted_path, lexical_normalize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Filesystem abstraction covering file reads, existence checks, directory listings, and recursive
/// source-tree walks.
pub trait FileSource: Send + Sync {
    /// Reads UTF-8 text of `path`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `path` does not exist or is not valid UTF-8.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    /// Returns true if `path` exists as a regular file.
    fn is_file(&self, path: &Path) -> bool;

    /// Returns true if `path` exists as a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// Lists direct child paths inside `directory` in deterministic sorted order.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `directory` cannot be listed.
    fn read_dir(&self, directory: &Path) -> io::Result<Vec<PathBuf>>;

    /// Recursively lists all non-ignored regular files under `directory` in deterministic sorted
    /// order, skipping hidden entries (`.git`, `.jj`, etc.) and common build/virtualenv
    /// directories (`target`, `__pycache__`, `node_modules`, `venv`).
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `directory` does not exist or cannot be traversed.
    fn walk_files(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        if !self.is_dir(directory) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                directory.display().to_string(),
            ));
        }
        let mut files = Vec::new();
        walk_file_source_recursive(self, directory, &mut files);
        files.sort();
        files.dedup();
        Ok(files)
    }

    /// Returns the working directory used to resolve relative paths against this source, if any.
    fn current_dir(&self) -> Option<PathBuf>;
}

/// Real OS filesystem implementation of [`FileSource`].
#[derive(Debug, Default, Clone, Copy)]
pub struct DiskFileSource;

impl FileSource for DiskFileSource {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_dir(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        let mut entries: Vec<PathBuf> = fs::read_dir(directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        Ok(entries)
    }

    fn walk_files(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        if !self.is_dir(directory) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                directory.display().to_string(),
            ));
        }
        let mut files: Vec<PathBuf> = ignore::WalkBuilder::new(directory)
            .require_git(false)
            .filter_entry(|entry| entry.depth() == 0 || !should_skip_walk_entry(entry.file_name()))
            .build()
            .filter_map(Result::ok)
            .map(ignore::DirEntry::into_path)
            .filter(|path| path.is_file())
            .collect();
        files.sort();
        files.dedup();
        Ok(files)
    }

    fn current_dir(&self) -> Option<PathBuf> {
        std::env::current_dir().ok()
    }
}

/// Hermetic in-memory implementation of [`FileSource`] for `rule_test!` and unit tests.
#[derive(Debug, Clone)]
pub struct MemoryFileSource {
    root: PathBuf,
    files: HashMap<PathBuf, String>,
    directories: HashSet<PathBuf>,
}

impl Default for MemoryFileSource {
    fn default() -> Self {
        let root = lexical_normalize(Path::new(Self::VIRTUAL_ROOT));
        Self {
            directories: HashSet::from([root.clone()]),
            root,
            files: HashMap::new(),
        }
    }
}

impl MemoryFileSource {
    /// Canonical virtual root path for hermetic in-memory projects.
    pub const VIRTUAL_ROOT: &'static str = "/proj";

    /// Builds an in-memory filesystem rooted at `root` from `(relative_path, content)` pairs.
    #[must_use]
    pub fn from_relative_files(root: &Path, entries: &[(&str, &str)]) -> Self {
        let norm_root = lexical_normalize(root);
        let mut files = HashMap::with_capacity(entries.len());
        let mut directories = HashSet::new();
        directories.insert(norm_root.clone());
        for &(relative, content) in entries {
            let full = lexical_normalize(&norm_root.join(relative));
            let mut ancestor = full.parent();
            while let Some(directory) = ancestor {
                directories.insert(directory.to_path_buf());
                if directory == norm_root {
                    break;
                }
                ancestor = directory.parent();
            }
            files.insert(full, content.to_owned());
        }
        Self {
            root: norm_root,
            files,
            directories,
        }
    }

    fn resolve_path(&self, path: &Path) -> PathBuf {
        if is_rooted_path(path) {
            lexical_normalize(path)
        } else {
            lexical_normalize(&self.root.join(path))
        }
    }
}

impl FileSource for MemoryFileSource {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let norm = self.resolve_path(path);
        self.files
            .get(&norm)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(&self.resolve_path(path))
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.directories.contains(&self.resolve_path(path))
    }

    fn read_dir(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        let norm_dir = self.resolve_path(directory);
        if !self.directories.contains(&norm_dir) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                directory.display().to_string(),
            ));
        }
        let mut children = BTreeSet::new();
        for file_path in self.files.keys() {
            if let Ok(relative) = file_path.strip_prefix(&norm_dir)
                && let Some(first) = relative.components().next()
            {
                children.insert(norm_dir.join(first.as_os_str()));
            }
        }
        Ok(children.into_iter().collect())
    }

    fn current_dir(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
}

/// Overlay [`FileSource`] serving unsaved file buffers on top of a backing [`FileSource`].
#[derive(Debug, Clone)]
pub struct OverlayFileSource<B> {
    base: B,
    overlays: HashMap<PathBuf, String>,
    overlay_directories: HashSet<PathBuf>,
}

impl<B: FileSource> OverlayFileSource<B> {
    /// Wraps `base` with in-memory `overlays` keyed by path.
    #[must_use]
    pub fn new(base: B, raw_overlays: HashMap<PathBuf, String>) -> Self {
        let cwd = base.current_dir();
        let mut overlays = HashMap::with_capacity(raw_overlays.len());
        let mut overlay_directories = HashSet::new();
        for (path, content) in raw_overlays {
            let norm = resolve_against_cwd(&path, cwd.as_deref());
            let mut parent = norm.parent();
            while let Some(directory) = parent {
                overlay_directories.insert(directory.to_path_buf());
                parent = directory.parent();
            }
            overlays.insert(norm, content);
        }
        Self {
            base,
            overlays,
            overlay_directories,
        }
    }

    fn resolve_path(&self, path: &Path) -> PathBuf {
        let cwd = self.base.current_dir();
        resolve_against_cwd(path, cwd.as_deref())
    }
}

impl<B: FileSource> FileSource for OverlayFileSource<B> {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let norm = self.resolve_path(path);
        if let Some(content) = self.overlays.get(&norm) {
            return Ok(content.clone());
        }
        self.base.read_to_string(&norm)
    }

    fn is_file(&self, path: &Path) -> bool {
        let norm = self.resolve_path(path);
        self.overlays.contains_key(&norm) || self.base.is_file(&norm)
    }

    fn is_dir(&self, path: &Path) -> bool {
        let norm = self.resolve_path(path);
        self.overlay_directories.contains(&norm) || self.base.is_dir(&norm)
    }

    fn read_dir(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        let norm_dir = self.resolve_path(directory);
        let mut children: BTreeSet<PathBuf> = match self.base.read_dir(&norm_dir) {
            Ok(entries) => entries
                .into_iter()
                .map(|entry| self.resolve_path(&entry))
                .collect(),
            Err(_) if self.overlay_directories.contains(&norm_dir) => BTreeSet::new(),
            Err(io_error) => return Err(io_error),
        };
        for path in self.overlays.keys() {
            if let Ok(relative) = path.strip_prefix(&norm_dir)
                && let Some(first) = relative.components().next()
            {
                children.insert(norm_dir.join(first.as_os_str()));
            }
        }
        Ok(children.into_iter().collect())
    }

    fn walk_files(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        let norm_dir = self.resolve_path(directory);
        let mut files: BTreeSet<PathBuf> = match self.base.walk_files(&norm_dir) {
            Ok(entries) => entries
                .into_iter()
                .map(|entry| self.resolve_path(&entry))
                .collect(),
            Err(_) if self.overlay_directories.contains(&norm_dir) => BTreeSet::new(),
            Err(io_error) => return Err(io_error),
        };
        for path in self.overlays.keys() {
            if let Ok(relative) = path.strip_prefix(&norm_dir)
                && !relative.as_os_str().is_empty()
                && relative
                    .components()
                    .all(|component| !should_skip_walk_entry(component.as_os_str()))
            {
                files.insert(path.clone());
            }
        }
        Ok(files.into_iter().collect())
    }

    fn current_dir(&self) -> Option<PathBuf> {
        self.base.current_dir()
    }
}

fn resolve_against_cwd(path: &Path, cwd: Option<&Path>) -> PathBuf {
    if is_rooted_path(path) {
        lexical_normalize(path)
    } else if let Some(cwd) = cwd {
        lexical_normalize(&cwd.join(path))
    } else {
        lexical_normalize(path)
    }
}

fn walk_file_source_recursive<S: FileSource + ?Sized>(
    source: &S,
    directory: &Path,
    out: &mut Vec<PathBuf>,
) {
    let Ok(entries) = source.read_dir(directory) else {
        return;
    };
    for entry in entries {
        if entry.file_name().is_some_and(should_skip_walk_entry) {
            continue;
        }
        if source.is_dir(&entry) {
            walk_file_source_recursive(source, &entry, out);
        } else if source.is_file(&entry) {
            out.push(entry);
        }
    }
}

fn should_skip_walk_entry(file_name: &OsStr) -> bool {
    file_name.to_str().is_some_and(|name| {
        name.starts_with('.') || matches!(name, "target" | "__pycache__" | "node_modules" | "venv")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_and_overlay_file_sources_merge_files_and_directories() {
        let root = Path::new("/virtual/repo");
        let base = MemoryFileSource::from_relative_files(
            root,
            &[("pkg/__init__.py", "X = 1"), ("pkg/a.py", "A = 1")],
        );
        let overlay = OverlayFileSource::new(
            base,
            HashMap::from([
                (PathBuf::from("pkg/a.py"), "A = 2".to_owned()),
                (root.join("pkg/sub/b.py"), "B = 3".to_owned()),
            ]),
        );

        assert_eq!(overlay.current_dir(), Some(lexical_normalize(root)));
        assert_eq!(
            overlay.read_to_string(Path::new("pkg/a.py")).unwrap(),
            "A = 2"
        );
        assert!(overlay.is_dir(Path::new("pkg/sub")));
        assert_eq!(
            overlay.read_dir(Path::new("pkg")).unwrap(),
            vec![
                root.join("pkg/__init__.py"),
                root.join("pkg/a.py"),
                root.join("pkg/sub"),
            ]
        );
    }

    #[test]
    fn walk_files_skips_ignored_directories_and_includes_overlays() {
        let root = Path::new("/virtual/repo");
        let base = MemoryFileSource::from_relative_files(
            root,
            &[
                (".git/config", "ignored"),
                ("target/debug/out.rs", "ignored"),
                ("pkg/__init__.py", "X = 1"),
                ("pkg/a.py", "A = 1"),
            ],
        );
        let overlay = OverlayFileSource::new(
            base,
            HashMap::from([(PathBuf::from("pkg/sub/b.py"), "B = 3".to_owned())]),
        );

        assert_eq!(
            overlay.walk_files(Path::new(".")).unwrap(),
            vec![
                root.join("pkg/__init__.py"),
                root.join("pkg/a.py"),
                root.join("pkg/sub/b.py"),
            ]
        );
        assert_eq!(
            overlay.walk_files(Path::new("pkg/sub")).unwrap(),
            vec![root.join("pkg/sub/b.py")]
        );
        assert!(overlay.walk_files(Path::new("nonexistent")).is_err());
    }
}
