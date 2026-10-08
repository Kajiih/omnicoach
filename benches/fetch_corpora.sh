#!/usr/bin/env bash
# Fetches the pinned end-to-end benchmark corpora into target/bench-corpora/.
#
# The projects follow SOTA practice; the versions are the latest releases as of 2026-10-08
# (bump them deliberately, since results are only comparable across runs on the same tags):
# - cpython v3.14.8: Ruff's end-to-end benchmark project ("linting CPython from scratch").
# - ripgrep 15.2.0, cargo 0.100.0: from Clippy's lintcheck crate list (lintcheck_crates.toml).
#
# Usage: bash benches/fetch_corpora.sh
# Then:  hyperfine -N -i "target/release/omni-code-lint target/bench-corpora/cpython"
set -euo pipefail

DEST="$(dirname "$0")/../target/bench-corpora"
mkdir -p "$DEST"

fetch() {
  local name="$1" url="$2" tag="$3"
  if [[ -d "$DEST/$name" ]] && git -C "$DEST/$name" describe --tags --exact-match 2>/dev/null | grep -qx "$tag"; then
    echo "$name: $tag already present"
    return
  fi
  rm -rf "${DEST:?}/$name"
  git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$tag" "$url" "$DEST/$name"
  echo "$name: fetched $tag"
}

fetch cpython https://github.com/python/cpython.git v3.14.8
fetch ripgrep https://github.com/BurntSushi/ripgrep.git 15.2.0
fetch cargo https://github.com/rust-lang/cargo.git 0.100.0
