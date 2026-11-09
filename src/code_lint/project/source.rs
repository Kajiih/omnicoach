//! Filesystem abstraction for disk, hermetic in-memory tests, and unsaved hook buffer overlays.

use crate::config::lexical_normalize;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Filesystem abstraction covering file reads, existence checks, and directory listings.
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
}

impl FileSource for MemoryFileSource {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let norm = lexical_normalize(path);
        self.files
            .get(&norm)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(&lexical_normalize(path))
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.directories.contains(&lexical_normalize(path))
    }

    fn read_dir(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        let norm_dir = lexical_normalize(directory);
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
        let mut overlays = HashMap::with_capacity(raw_overlays.len());
        let mut overlay_directories = HashSet::new();
        for (path, content) in raw_overlays {
            let norm = lexical_normalize(&path);
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
}

impl<B: FileSource> FileSource for OverlayFileSource<B> {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let norm = lexical_normalize(path);
        if let Some(content) = self.overlays.get(&norm) {
            return Ok(content.clone());
        }
        self.base.read_to_string(&norm)
    }

    fn is_file(&self, path: &Path) -> bool {
        let norm = lexical_normalize(path);
        self.overlays.contains_key(&norm) || self.base.is_file(&norm)
    }

    fn is_dir(&self, path: &Path) -> bool {
        let norm = lexical_normalize(path);
        self.overlay_directories.contains(&norm) || self.base.is_dir(&norm)
    }

    fn read_dir(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        let norm_dir = lexical_normalize(directory);
        let mut children: BTreeSet<PathBuf> = match self.base.read_dir(&norm_dir) {
            Ok(entries) => entries
                .into_iter()
                .map(|entry| lexical_normalize(&entry))
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

    fn current_dir(&self) -> Option<PathBuf> {
        self.base.current_dir()
    }
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
                (root.join("pkg/a.py"), "A = 2".to_owned()),
                (root.join("pkg/sub/b.py"), "B = 3".to_owned()),
            ]),
        );

        assert_eq!(overlay.current_dir(), Some(lexical_normalize(root)));
        assert_eq!(
            overlay.read_to_string(&root.join("pkg/a.py")).unwrap(),
            "A = 2"
        );
        assert!(overlay.is_dir(&root.join("pkg/sub")));
        assert_eq!(
            overlay.read_dir(&root.join("pkg")).unwrap(),
            vec![
                root.join("pkg/__init__.py"),
                root.join("pkg/a.py"),
                root.join("pkg/sub"),
            ]
        );
    }
}
