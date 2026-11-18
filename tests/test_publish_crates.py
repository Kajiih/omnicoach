"""Hermetic unit tests for `scripts/publish_crates.py`."""

from __future__ import annotations

import contextlib
import dataclasses
import http.server
import inspect
import json
import os
import pathlib
import sys
import tempfile
import threading
import unittest
from collections.abc import Mapping, Sequence

from scripts import publish_crates

LOOPBACK_NO_PROXY = "127.0.0.1,localhost"
FAKE_METADATA = {
    "workspace_members": [
        "path+file:///repo/crates/tagonomy#0.0.1",
        "path+file:///repo#omnicoach@0.2.0",
        "path+file:///repo/crates/omnicoach-py#0.2.0",
        "path+file:///repo/crates/internal#0.1.0",
    ],
    "packages": [
        {
            "id": "path+file:///repo/crates/tagonomy#0.0.1",
            "name": "tagonomy",
            "version": "0.0.1",
            "publish": None,
        },
        {
            "id": "path+file:///repo#omnicoach@0.2.0",
            "name": "omnicoach",
            "version": "0.2.0",
            "publish": ["crates-io"],
        },
        {
            "id": "path+file:///repo/crates/omnicoach-py#0.2.0",
            "name": "omnicoach-py",
            "version": "0.2.0",
            "publish": [],
        },
        {
            "id": "path+file:///repo/crates/internal#0.1.0",
            "name": "internal",
            "version": "0.1.0",
            "publish": ["private-registry"],
        },
        {
            "id": "registry+https://github.com/rust-lang/crates.io-index#fixedbitset@0.5.7",
            "name": "fixedbitset",
            "version": "0.5.7",
            "publish": None,
        },
    ],
}


class PublishCratesTest(unittest.TestCase):
    """End-to-end behavior of `publish_crates.main` against a fake cargo and crates.io server."""

    def test_publishes_workspace_when_no_versions_exist(self) -> None:
        with _harness(
            {"/crates/tagonomy/0.0.1": 404, "/crates/omnicoach/0.2.0": 404}
        ) as env:
            exit_code = env.run_publisher(["--", "--locked"])

            self.assertEqual(exit_code, 0)
            self.assertEqual(
                env.cargo_calls(),
                [
                    ["metadata", "--format-version", "1", "--no-deps"],
                    ["publish", "--workspace", "--exclude", "internal", "--locked"],
                ],
            )
            self.assertEqual(
                env.requested_paths,
                ["/crates/tagonomy/0.0.1", "/crates/omnicoach/0.2.0"],
            )
            self.assertTrue(
                all("omnicoach-publish" in agent for agent in env.user_agents)
            )

    def test_excludes_already_published_crate_on_retry(self) -> None:
        with _harness(
            {"/crates/tagonomy/0.0.1": 200, "/crates/omnicoach/0.2.0": 404}
        ) as env:
            exit_code = env.run_publisher(["--", "--locked"])

            self.assertEqual(exit_code, 0)
            self.assertEqual(
                env.cargo_calls(),
                [
                    ["metadata", "--format-version", "1", "--no-deps"],
                    [
                        "publish",
                        "--workspace",
                        "--exclude",
                        "internal",
                        "--exclude",
                        "tagonomy",
                        "--locked",
                    ],
                ],
            )

    def test_skips_cargo_publish_when_all_crates_exist(self) -> None:
        with _harness(
            {"/crates/tagonomy/0.0.1": 200, "/crates/omnicoach/0.2.0": 200}
        ) as env:
            exit_code = env.run_publisher(["--", "--locked"])

            self.assertEqual(exit_code, 0)
            self.assertEqual(
                env.cargo_calls(),
                [["metadata", "--format-version", "1", "--no-deps"]],
            )

    def test_propagates_cargo_publish_exit_code(self) -> None:
        with _harness(
            {"/crates/tagonomy/0.0.1": 404, "/crates/omnicoach/0.2.0": 404},
            publish_exit_code=101,
        ) as env:
            exit_code = env.run_publisher(["--", "--locked"])

            self.assertEqual(exit_code, 101)

    def test_raises_on_crates_io_server_error_without_publishing(self) -> None:
        with _harness({"/crates/tagonomy/0.0.1": 500}) as env:
            with self.assertRaisesRegex(RuntimeError, r"tagonomy@0\.0\.1.*HTTP 500"):
                env.run_publisher(["--", "--locked"])

            self.assertEqual(
                env.cargo_calls(),
                [["metadata", "--format-version", "1", "--no-deps"]],
            )


@dataclasses.dataclass(frozen=True, slots=True)
class _Harness:
    """Running fake crates.io HTTP server and fake `cargo` script."""

    cargo_script: pathlib.Path
    calls_file: pathlib.Path
    api_url: str
    requested_paths: Sequence[str]
    user_agents: Sequence[str]

    def run_publisher(self, extra_args: Sequence[str]) -> int:
        """Invoke `publish_crates.main` wired to this harness."""
        return publish_crates.main(
            [
                "--cargo",
                sys.executable,
                str(self.cargo_script),
                "--api-url",
                self.api_url,
                "--request-delay-ms",
                "0",
                *extra_args,
            ]
        )

    def cargo_calls(self) -> Sequence[Sequence[str]]:
        """Return every argument vector passed to the fake `cargo` script."""
        if not self.calls_file.exists():
            return []
        lines = self.calls_file.read_text(encoding="utf-8").splitlines()
        return [json.loads(line) for line in lines]


@contextlib.contextmanager
def _harness(routes: Mapping[str, int], *, publish_exit_code: int = 0):
    """Start a loopback HTTP server and temporary fake `cargo` script.

    Args:
        routes: Map from request path (`/crates/<name>/<version>`) to HTTP status code.
        publish_exit_code: Exit code returned by the fake `cargo` script on `publish` invocations.
    """
    requested_paths: list[str] = []
    user_agents: list[str] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            requested_paths.append(self.path)
            user_agents.append(self.headers.get("User-Agent", ""))
            status = routes.get(self.path, 404)
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b"{}")

        def log_message(self, _format: str, *_args: object) -> None:
            return

    previous_no_proxy = os.environ.get("NO_PROXY")
    previous_lower_no_proxy = os.environ.get("no_proxy")
    os.environ["NO_PROXY"] = LOOPBACK_NO_PROXY
    os.environ["no_proxy"] = LOOPBACK_NO_PROXY
    try:
        with (
            tempfile.TemporaryDirectory(prefix="test-publish-crates-") as temp_dir,
            http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server,
        ):
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                root = pathlib.Path(temp_dir)
                calls_file = root / "cargo_calls.jsonl"
                cargo_script = _write_fake_cargo(root, calls_file, publish_exit_code)
                host, port = server.server_address[:2]
                yield _Harness(
                    cargo_script=cargo_script,
                    calls_file=calls_file,
                    api_url=f"http://{host}:{port}",
                    requested_paths=requested_paths,
                    user_agents=user_agents,
                )
            finally:
                server.shutdown()
                thread.join(timeout=5)
    finally:
        _restore_env("NO_PROXY", previous_no_proxy)
        _restore_env("no_proxy", previous_lower_no_proxy)


def _restore_env(key: str, value: str | None) -> None:
    """Restore or unset `os.environ[key]`."""
    if value is None:
        os.environ.pop(key, None)
    else:
        os.environ[key] = value


def _write_fake_cargo(
    root: pathlib.Path, calls_file: pathlib.Path, publish_exit_code: int
) -> pathlib.Path:
    """Write a portable Python script acting as `cargo`."""
    metadata_path = root / "metadata.json"
    metadata_path.write_text(json.dumps(FAKE_METADATA), encoding="utf-8")
    cargo_script = root / "fake_cargo.py"
    script_body = inspect.cleandoc(
        f"""
        import json
        import pathlib
        import sys

        calls = pathlib.Path({str(calls_file)!r})
        with calls.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(sys.argv[1:]) + "\\n")
        if sys.argv[1:2] == ["metadata"]:
            sys.stdout.write(pathlib.Path({str(metadata_path)!r}).read_text(encoding="utf-8"))
            sys.exit(0)
        sys.exit({publish_exit_code})
        """
    )
    cargo_script.write_text(script_body + "\n", encoding="utf-8")
    return cargo_script


if __name__ == "__main__":
    unittest.main()
