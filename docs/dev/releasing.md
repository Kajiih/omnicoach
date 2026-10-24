# Releasing a Version

A release is one commit that only bumps the crate version, tagged `vX.Y.Z` and pushed with `main`. There is no changelog: the commit history since the previous tag is the record.

## Choosing the version

Until 1.0, bump the minor version (`0.2.0` → `0.3.0`) when the release adds rules or changes behavior, the CLI, the config schema, or the library API. Bump the patch version only for releases that contain nothing but fixes.

## Steps

Start from an empty working copy on top of the last commit to release, with every commit to ship already on `main`.

1. Verify: `cargo test` and `cargo clippy --all-targets` pass.
2. Bump the version on the `version` line of `[package]` in `Cargo.toml`, then refresh the lockfile:

   ```sh
   cargo update -p omnicoach --offline
   jj diff --stat   # only Cargo.toml and Cargo.lock change
   ```

3. Commit, move `main`, then tag:

   ```sh
   jj commit -m "Release X.Y.Z"
   jj bookmark set main -r @-
   jj tag set vX.Y.Z -r @-
   ```

   Tag last, after `jj commit`. A tagged commit is immutable, so tagging the working-copy commit makes jj start a new working copy, and a later `jj bookmark set main -r @` then lands on that empty commit instead of the release.
4. Push the bookmark and the tag. `jj git push` pushes bookmarks but not tags, so push the tag with git:

   ```sh
   jj git push --bookmark main
   git push origin vX.Y.Z
   ```

5. Check that `main@origin` and `vX.Y.Z` point to the same commit: `jj log -r 'main@origin | vX.Y.Z'`.
