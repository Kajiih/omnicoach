#!/usr/bin/env python3
"""Publish workspace crates to crates.io idempotently.

`cargo publish --workspace` aborts if any publishable workspace crate version already exists on
crates.io, which breaks release retries after a partial publish. This script queries crates.io
first, passes `--exclude <name>` for every workspace crate whose version is already published or
restricted to a non-crates.io registry, and delegates topological ordering and publishing of the
remaining crates to a single `cargo publish --workspace` invocation.
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime
import http.client
import json
import pathlib
import subprocess
import sys
import time
import typing
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Iterable, Sequence
from collections.abc import Set as AbstractSet

CRATES_IO_API = "https://crates.io/api/v1"
USER_AGENT = "omnicoach-publish (https://github.com/Kajiih/omnicoach)"
REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_REQUEST_DELAY = datetime.timedelta(seconds=1)
HTTP_TIMEOUT = datetime.timedelta(seconds=30)
METADATA_TIMEOUT = datetime.timedelta(minutes=2)
PUBLISH_TIMEOUT = datetime.timedelta(minutes=15)
ALLOWED_SCHEMES = frozenset({"http", "https"})


@dataclasses.dataclass(frozen=True, slots=True)
class Crate:
    """Publishable workspace crate name and version."""

    name: str
    version: str

    def pretty(self) -> str:
        """Format as `name@version` for human-readable status logs."""
        return f"{self.name}@{self.version}"


class PackageMetadata(typing.TypedDict):
    """Subset of a package entry in `cargo metadata --format-version 1`."""

    id: str
    name: str
    version: str
    publish: Sequence[str] | None


class CargoMetadata(typing.TypedDict):
    """Subset of `cargo metadata --format-version 1 --no-deps` output."""

    workspace_members: Sequence[str]
    packages: Sequence[PackageMetadata]


@dataclasses.dataclass(frozen=True, slots=True)
class Options:
    """Parsed command-line options for `publish_crates.py`."""

    cargo: Sequence[str]
    api_url: str
    request_delay: datetime.timedelta
    cargo_publish_args: Sequence[str]


def main(argv: Sequence[str] | None = None) -> int:
    """Query crates.io and publish any missing workspace crate versions.

    Args:
        argv: Explicit CLI argument vector for programmatic callers; defaults to `sys.argv[1:]`.
    """
    options = _parse_args(argv)
    workspace = _workspace_crates(options.cargo)
    existing = _existing_crate_versions(
        workspace.crates_io,
        api_url=options.api_url,
        delay=options.request_delay,
    )
    missing = [crate for crate in workspace.crates_io if crate.name not in existing]
    if not missing:
        print("All publishable workspace crate versions already exist on crates.io")
        return 0

    print("Publishing workspace crate versions missing from crates.io:")
    for crate in missing:
        print(f"  {crate.pretty()}")

    excluded = existing | workspace.other_registry_names
    command = [*options.cargo, "publish", "--workspace"]
    for crate_name in sorted(excluded):
        command.extend(["--exclude", crate_name])
    command.extend(options.cargo_publish_args)
    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        stdin=subprocess.DEVNULL,
        timeout=PUBLISH_TIMEOUT.total_seconds(),
        check=False,
    )
    return completed.returncode


def _parse_args(argv: Sequence[str] | None) -> Options:
    """Parse internal test overrides and trailing `cargo publish` arguments."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--cargo", default=["cargo"], nargs="+", help=argparse.SUPPRESS)
    parser.add_argument("--api-url", default=CRATES_IO_API, help=argparse.SUPPRESS)
    parser.add_argument(
        "--request-delay-ms",
        type=_parse_milliseconds,
        default=DEFAULT_REQUEST_DELAY,
        dest="request_delay",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "cargo_publish_args",
        nargs=argparse.REMAINDER,
        help="Extra flags forwarded to `cargo publish --workspace` after `--`.",
    )
    parsed = parser.parse_args(argv)
    publish_args = list(parsed.cargo_publish_args)
    if publish_args[:1] == ["--"]:
        publish_args = publish_args[1:]
    return Options(
        cargo=tuple(parsed.cargo),
        api_url=parsed.api_url.rstrip("/"),
        request_delay=parsed.request_delay,
        cargo_publish_args=tuple(publish_args),
    )


def _parse_milliseconds(raw: str) -> datetime.timedelta:
    """Convert an integer millisecond CLI argument into a `timedelta`."""
    return datetime.timedelta(milliseconds=int(raw))


@dataclasses.dataclass(frozen=True, slots=True)
class _WorkspaceCrates:
    """Workspace members split into crates.io targets and other-registry exclusions."""

    crates_io: Sequence[Crate]
    other_registry_names: AbstractSet[str]


def _workspace_crates(cargo: Iterable[str]) -> _WorkspaceCrates:
    """Split workspace members into crates.io targets and non-crates.io registry exclusions."""
    completed = subprocess.run(
        [*cargo, "metadata", "--format-version", "1", "--no-deps"],
        cwd=REPO_ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        timeout=METADATA_TIMEOUT.total_seconds(),
        check=True,
    )
    metadata: CargoMetadata = json.loads(completed.stdout)
    workspace_members = frozenset(metadata["workspace_members"])
    crates_io: list[Crate] = []
    other_registry_names: set[str] = set()
    for package in metadata["packages"]:
        if package["id"] not in workspace_members:
            continue
        publish = package["publish"]
        if publish is None or "crates-io" in publish:
            crates_io.append(Crate(name=package["name"], version=package["version"]))
        elif publish:
            other_registry_names.add(package["name"])
    return _WorkspaceCrates(
        crates_io=tuple(crates_io),
        other_registry_names=frozenset(other_registry_names),
    )


def _existing_crate_versions(
    crates: Iterable[Crate],
    *,
    api_url: str,
    delay: datetime.timedelta,
) -> AbstractSet[str]:
    """Return names of `crates` whose `name@version` already exists on crates.io."""
    existing: set[str] = set()
    for index, crate in enumerate(crates):
        if index > 0 and delay.total_seconds() > 0:
            time.sleep(delay.total_seconds())
        if _crate_version_exists(crate, api_url):
            print(f"Skipping {crate.pretty()}: already exists on crates.io")
            existing.add(crate.name)
    return frozenset(existing)


def _crate_version_exists(crate: Crate, api_url: str) -> bool:
    """Query `{api_url}/crates/{name}/{version}` on crates.io.

    Returns:
        `True` when HTTP 200 confirms the crate version is already published, or `False` when HTTP
        404 indicates the version has not been published yet.
    """
    crate_name = urllib.parse.quote(crate.name, safe="")
    crate_version = urllib.parse.quote(crate.version, safe="")
    url = f"{api_url}/crates/{crate_name}/{crate_version}"
    if urllib.parse.urlsplit(url).scheme not in ALLOWED_SCHEMES:
        raise ValueError(f"unsupported crates.io API URL scheme in {url!r}")
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(
            request, timeout=HTTP_TIMEOUT.total_seconds()
        ) as response:
            if response.status == 200:
                return True
            raise RuntimeError(
                f"unexpected HTTP {response.status} querying {crate.pretty()}"
            )
    except urllib.error.HTTPError as error:
        try:
            if error.code == 404:
                return False
            raise RuntimeError(
                f"failed to query {crate.pretty()} from crates.io: HTTP {error.code}"
            ) from error
        finally:
            error.close()
    except (OSError, http.client.HTTPException) as error:
        raise RuntimeError(
            f"failed to query {crate.pretty()} from crates.io: {error}"
        ) from error


if __name__ == "__main__":
    sys.exit(main())
