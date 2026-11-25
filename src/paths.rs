//! Host-independent lexical path normalization and rooted-path classification utilities.

architecture_component!(Paths);

use std::path::{Component, Path, PathBuf};

/// Normalizes `path` lexically by stripping `.` and resolving `..` segments without touching the
/// filesystem, returning `"."` when all segments collapse.
#[must_use]
pub fn lexical_normalize(path: &Path) -> PathBuf {
    path_clean::clean(path)
}

/// Returns `true` if `path` is anchored at a root separator or drive/UNC prefix rather than
/// directory-relative.
///
/// Unlike [`Path::is_absolute`], which returns `false` on Windows for drive-less rooted paths
/// such as `"\repo"`, `is_rooted_path` matches all paths that should bypass `cwd` anchoring,
/// including Unix and Windows root separators as well as Windows drive and UNC prefixes.
#[must_use]
pub fn is_rooted_path(path: &Path) -> bool {
    path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_)))
}

/// Resolves `path` against `cwd` when `path` is relative, normalizing `.` and `..` lexically.
#[must_use]
pub fn resolve_against_cwd(path: &Path, cwd: Option<&Path>) -> PathBuf {
    cwd.filter(|_| !is_rooted_path(path)).map_or_else(
        || lexical_normalize(path),
        |base| lexical_normalize(&base.join(path)),
    )
}

/// Returns `true` when lexically normalized `candidate` is equal to or nested beneath lexically
/// normalized `directory`.
#[must_use]
pub fn path_is_within(candidate: &Path, directory: &Path) -> bool {
    let candidate = lexical_normalize(candidate);
    let directory = lexical_normalize(directory);
    if let Some(dir_parent_depth) = ancestor_only_depth(&directory) {
        return !is_rooted_path(&candidate) && leading_parent_depth(&candidate) <= dir_parent_depth;
    }
    candidate.starts_with(&directory)
}

fn ancestor_only_depth(normalized: &Path) -> Option<usize> {
    if normalized == Path::new(".") {
        return Some(0);
    }
    let mut depth = 0usize;
    for component in normalized.components() {
        if component != Component::ParentDir {
            return None;
        }
        depth += 1;
    }
    (depth > 0).then_some(depth)
}

fn leading_parent_depth(normalized: &Path) -> usize {
    normalized
        .components()
        .take_while(|component| *component == Component::ParentDir)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::empty("", ".")]
    #[case::cur_dir(".", ".")]
    #[case::parent_dir("..", "..")]
    #[case::rooted_parent("/../a", "/a")]
    #[case::repeated_slashes("/a//b///c", "/a/b/c")]
    #[cfg_attr(not(windows), case::leading_double_slash("//a//b", "/a/b"))]
    #[case::relative_parent_preserved("../a/b", "../a/b")]
    #[case::nested_parent_collapsed("a/b/../c/.", "a/c")]
    fn test_lexical_normalize(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(lexical_normalize(Path::new(input)), PathBuf::from(expected));
    }

    #[rstest::rstest]
    #[case::unix_root("/", true)]
    #[case::unix_anchored("/repo/src/lib.rs", true)]
    #[case::relative_file("src/lib.rs", false)]
    #[case::dot_relative("./src/lib.rs", false)]
    #[case::parent_relative("../src/lib.rs", false)]
    #[case::empty("", false)]
    #[cfg_attr(windows, case::windows_drive_root(r"C:\repo\src\lib.rs", true))]
    #[cfg_attr(windows, case::windows_drive_relative(r"C:src\lib.rs", true))]
    #[cfg_attr(windows, case::windows_unc(r"\\server\share\repo\lib.rs", true))]
    fn test_is_rooted_path(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(is_rooted_path(Path::new(input)), expected);
    }

    #[rstest::rstest]
    #[case::relative_from_cwd("src/../tests", Some("/repo"), "/repo/tests")]
    #[case::empty_with_cwd("", Some("/repo"), "/repo")]
    #[case::rooted_ignores_cwd("/other/../tmp", Some("/repo"), "/tmp")]
    #[cfg_attr(
        windows,
        case::windows_driveless_root_ignores_cwd(r"\other\..\tmp", Some(r"C:\repo"), r"\tmp")
    )]
    #[case::no_cwd("../a/./b", None, "../a/b")]
    fn test_resolve_against_cwd(
        #[case] input: &str,
        #[case] cwd: Option<&str>,
        #[case] expected: &str,
    ) {
        assert_eq!(
            resolve_against_cwd(Path::new(input), cwd.map(Path::new)),
            PathBuf::from(expected)
        );
    }

    #[rstest::rstest]
    #[case::exact_dir("/workspace", "/workspace", true)]
    #[case::nested_file("/workspace/out.txt", "/workspace", true)]
    #[case::normalized_subdir("/workspace/sub/../out.txt", "/workspace", true)]
    #[case::parent_escape("/workspace/../other/out.txt", "/workspace", false)]
    #[case::prefix_confusion("/workspace_other/out.txt", "/workspace", false)]
    #[case::dot_dir_relative("sub/out.txt", ".", true)]
    #[case::dot_dir_exact(".", ".", true)]
    #[case::dot_dir_parent_escape("../out.txt", ".", false)]
    #[case::dot_dir_rooted("/tmp/out.txt", ".", false)]
    #[case::root_dir_contains_rooted("/workspace/out.txt", "/", true)]
    #[case::parent_dir_exact("..", "..", true)]
    #[case::parent_dir_nested("../workspace/out.txt", "..", true)]
    #[case::parent_dir_escape("../../out.txt", "..", false)]
    #[case::double_parent_dir_escape("../../../out.txt", "../..", false)]
    #[case::parent_dir_contains_cur_dir(".", "..", true)]
    #[case::parent_dir_contains_cwd_relative("workspace/out.txt", "..", true)]
    #[case::double_parent_dir_contains_shallower_parent("../workspace/out.txt", "../..", true)]
    #[case::parent_dir_rejects_rooted("/tmp/out.txt", "..", false)]
    #[case::sibling_parent_dir_rejects_cwd_relative("workspace/out.txt", "../workspace", false)]
    fn test_path_is_within(
        #[case] candidate: &str,
        #[case] directory: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(
            path_is_within(Path::new(candidate), Path::new(directory)),
            expected
        );
    }
}
