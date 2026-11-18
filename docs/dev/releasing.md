# Releasing a Version

A release is one commit that only bumps the crate version, tagged `vX.Y.Z` and pushed with `main`. There is no changelog: the commit history since the previous tag is the record.

## Choosing the version

Until 1.0, bump the minor version (`0.2.0` → `0.3.0`) when the release adds rules or changes behavior, the CLI, the config schema, or the library API. Bump the patch version only for releases that contain nothing but fixes.

## Steps

Start from an empty working copy on top of the last commit to release, with every commit to ship already on `main`.

1. Verify: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo publish --workspace --dry-run --locked` pass.
2. Bump `version` under `[workspace.package]` in root `Cargo.toml` (shared by `omnicoach` and `crates/omnicoach-py`). When `crates/tagonomy` also changed in the release, bump its independent `version` in `crates/tagonomy/Cargo.toml` and the `tagonomy` dependency requirement in root `Cargo.toml`, then refresh the workspace entries in `Cargo.lock`:

   ```sh
   cargo update --workspace --offline
   jj diff --stat   # only Cargo.toml files and Cargo.lock change
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

5. Check that `main@origin` and `vX.Y.Z` point to the same commit: `jj log -r 'main@origin | vX.Y.Z'`. Pushing `vX.Y.Z` triggers [release.yml](../../.github/workflows/release.yml), which publishes the wheels and sdist to PyPI, standalone binaries and `omnicoach_playground.wasm` to GitHub Releases, and missing workspace crate versions (`tagonomy`, `omnicoach`) to crates.io via [publish_crates.py](../../scripts/publish_crates.py), then dispatches [pages.yml](../../.github/workflows/pages.yml).

## Initial crates.io Setup for a New Workspace Crate

crates.io requires a crate to exist before GitHub Actions OIDC Trusted Publishing can be registered on its settings page. Before pushing the first `vX.Y.Z` tag that ships a brand-new publishable workspace crate (such as `tagonomy` `0.0.1`), publish `0.0.1` once manually with a short-lived `publish-new` API token (`cargo publish -p tagonomy --locked`), register `Kajiih/omnicoach` (`release.yml`, environment `crates-io`) under the crate's Trusted Publishing settings on crates.io, and revoke the manual token so `rust-lang/crates-io-auth-action` mints OIDC tokens covering every workspace crate.
