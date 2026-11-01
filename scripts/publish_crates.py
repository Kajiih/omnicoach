#!/usr/bin/env python3
# Publish workspace crates to crates.io idempotently.
#
# `cargo publish --workspace` fails if any selected crate version already exists on crates.io. That
# makes release re-runs fail after a previous partial publish. This script queries crates.io first,
# excludes package versions that already exist, then delegates to `cargo publish --workspace` for the
# remaining packages so Cargo still handles package ordering.
#
# Usage:
#
#   CARGO_REGISTRY_TOKEN=<tok> python3 scripts/publish_crates.py

import argparse
import dataclasses
import datetime
import json
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Iterable, Sequence
from collections.abc import Set as AbstractSet

CRATES_IO_API = "https://crates.io/api/v1"
USER_AGENT = "omnicoach-publish (github.com/Kajiih/omnicoach)"
REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
CRATES_IO_REQUEST_DELAY = datetime.timedelta(milliseconds=200)
CARGO_PUBLISH_COMMAND = "publish"


@dataclasses.dataclass(frozen=True, slots=True)
class Crate:
    name: str
    version: str

    def pretty(self) -> str:
        return f"{self.name}@{self.version}"


def get_publishable_crates(cargo: Iterable[str]) -> Sequence[Crate]:
    """Return publishable workspace crates."""
    result = subprocess.run(
        [*cargo, "metadata", "--format-version", "1", "--no-deps"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    metadata = json.loads(result.stdout)
    workspace_member_ids = set(metadata["workspace_members"])
    crates = []

    for package in metadata["packages"]:
        if package["id"] not in workspace_member_ids:
            continue
        if package.get(CARGO_PUBLISH_COMMAND) == []:
            continue
        crates.append(Crate(name=package["name"], version=package["version"]))

    return crates


def crate_version_exists(crate: Crate, api_url: str) -> bool:
    """Return True if the crate version already exists on crates.io."""
    crate_name = urllib.parse.quote(crate.name, safe="")
    crate_version = urllib.parse.quote(crate.version, safe="")
    url = f"{api_url}/crates/{crate_name}/{crate_version}"

    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request) as response:
            if response.status == 200:
                return True
            raise RuntimeError(
                f"unexpected status {response.status} for {crate.pretty()}"
            )
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            return False
        raise RuntimeError(
            f"failed to query {crate.pretty()} from crates.io: HTTP {exc.code}"
        ) from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(
            f"failed to query {crate.pretty()} from crates.io: {exc}"
        ) from exc


def existing_crate_versions(crates: Iterable[Crate], api_url: str) -> AbstractSet[str]:
    """Return the names of workspace crates whose current version exists on crates.io."""
    existing = set()
    for index, crate in enumerate(crates):
        if index > 0:
            time.sleep(CRATES_IO_REQUEST_DELAY.total_seconds())
        if crate_version_exists(crate, api_url):
            print(f"Skipping {crate.pretty()}: already exists on crates.io")
            existing.add(crate.name)
    return frozenset(existing)


def main() -> int:
    """Entry point for the script that parses arguments and delegates to `cargo publish`."""
    parser = argparse.ArgumentParser(
        description="Publish workspace crates to crates.io idempotently."
    )
    parser.add_argument("--cargo", default=["cargo"], nargs="+", help=argparse.SUPPRESS)
    parser.add_argument(
        "cargo_publish_args", nargs=argparse.REMAINDER, help=argparse.SUPPRESS
    )
    args = parser.parse_args()

    if args.cargo_publish_args[:1] == ["--"]:
        args.cargo_publish_args = args.cargo_publish_args[1:]

    crates = get_publishable_crates(args.cargo)
    existing = existing_crate_versions(crates, CRATES_IO_API)
    missing = [crate for crate in crates if crate.name not in existing]

    if not missing:
        print("All publishable workspace crate versions already exist on crates.io")
        return 0

    print("Publishing workspace crate versions that do not exist on crates.io:")
    for crate in missing:
        print(f"  {crate.pretty()}")

    command = [*args.cargo, CARGO_PUBLISH_COMMAND, "--workspace"]
    for crate_name in sorted(existing):
        command.extend(["--exclude", crate_name])
    command.extend(args.cargo_publish_args)

    return subprocess.run(command, cwd=REPO_ROOT, check=False).returncode


if __name__ == "__main__":
    sys.exit(main())
