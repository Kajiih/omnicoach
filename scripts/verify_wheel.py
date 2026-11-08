#!/usr/bin/env python3
"""Verify the built omnicoach cp310-abi3 wheel and optional source distribution.

Installs the wheel into a fresh virtual environment per requested Python interpreter and checks
the CLI end to end through the console script: exit codes and output for each subcommand,
`python -m omnicoach` parity, Antigravity hook round-trips, a closed stdout pipe, and, when a
standalone binary is given, byte-for-byte parity with it.
"""

# omni:disable-file [repeated-literal] -- CLI arguments and hook wire keys read clearest inline; constants would only restate them

import argparse
import dataclasses
import datetime
import inspect
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import zipfile
from collections.abc import Iterable, Mapping, Sequence

MAX_WHEEL_BYTES = 5 * 1024 * 1024
TIMEOUT = datetime.timedelta(minutes=2)
SDIST_BUILD_TIMEOUT = datetime.timedelta(minutes=10)
RULE = "single-letter-name"
SAFE_COMMAND = "cargo test"
CLEAN_PYTHON = (
    inspect.cleandoc(
        """
        def compute_total() -> int:
            return 0
        """
    )
    + "\n"
)
VIOLATING_PYTHON = (
    inspect.cleandoc(
        """
        def compute_total(q: int) -> int:
            return q
        """
    )
    + "\n"
)
VIOLATING_RUST = (
    inspect.cleandoc(
        """
        pub fn compute_total(q: u64) -> u64 {
            q
        }
        """
    )
    + "\n"
)
FIXTURES = (
    ("clean.py", CLEAN_PYTHON),
    ("violating.py", VIOLATING_PYTHON),
    ("violating.rs", VIOLATING_RUST),
)
EXTENSION_ENTRIES = frozenset(
    {"omnicoach/_omnicoach.abi3.so", "omnicoach/_omnicoach.pyd"}
)
REQUIRED_ENTRIES = frozenset(
    {
        "omnicoach/__init__.py",
        "omnicoach/__main__.py",
        "omnicoach/_cli.py",
        "omnicoach/_omnicoach.pyi",
        "omnicoach/py.typed",
    }
)


@dataclasses.dataclass(frozen=True, slots=True)
class CliCase:
    """One CLI invocation (run from the fixtures directory) and its expected result."""

    args: Sequence[str]
    exit_code: int
    stdout_contains: str = ""


CLI_CASES = (
    CliCase(["--version"], 0, "omnicoach "),
    CliCase(["--help"], 0, "Usage: omnicoach"),
    CliCase(["--list-rules"], 0, RULE),
    CliCase(["--list-tags"], 0),
    CliCase(["--explain", RULE], 0, RULE),
    CliCase(["code", "--select", RULE, "clean.py"], 0),
    CliCase(["code", "--select", RULE, "violating.py"], 1, RULE),
    CliCase(["code", "--select", RULE, "violating.rs"], 1, RULE),
    CliCase(["code", "--format", "json", "--select", RULE, "violating.py"], 1, RULE),
    CliCase(["code", "missing.py"], 2),
    CliCase(["command", "--cmd", SAFE_COMMAND], 0),
    CliCase(["graph", "clean.py", "--depth", "module"], 0),
)
MODULE_PARITY_ARGS = (["--version"], ["--help"])
CLOSED_PIPE_CASES = (
    CliCase(["--help"], 0),
    CliCase(["--list-rules"], 0),
    CliCase(["code", "--format", "json", "--select", RULE, "violating.py"], 1),
)


@dataclasses.dataclass(frozen=True, slots=True)
class Outcome:
    """Exit status and captured streams of one process."""

    exit_code: int
    stdout: str
    stderr: str


@dataclasses.dataclass(frozen=True, slots=True)
class Venv:
    """A fresh virtual environment isolated from the user's Python and omnicoach config."""

    root: pathlib.Path
    env: Mapping[str, str]

    @classmethod
    def create(cls, root: pathlib.Path, interpreter: str) -> "Venv":
        """Create the environment under `root`, marking `root` as a repository boundary."""
        (root / ".git").mkdir()
        (root / "xdg").mkdir()
        env = {
            key: value
            for key, value in os.environ.items()
            if key not in {"PYTHONPATH", "PYTHONHOME", "VIRTUAL_ENV"}
        }
        env |= {"XDG_CONFIG_HOME": str(root / "xdg"), "NO_COLOR": "1"}
        _check(
            ["uv", "venv", "--python", interpreter, str(root / "venv")],
            cwd=root,
            env=env,
        )
        return cls(root=root, env=env)

    def executable(self, stem: str) -> pathlib.Path:
        """Return the path of console script or interpreter `stem` inside the environment."""
        scripts = ("Scripts", f"{stem}.exe") if os.name == "nt" else ("bin", stem)
        return self.root.joinpath("venv", *scripts)

    def install(
        self, artifact: pathlib.Path, extra_env: Mapping[str, str] | None = None
    ) -> None:
        """Install a wheel or sdist into the environment with `uv pip`.

        Args:
            artifact: Wheel or sdist to install; an sdist is built from source.
            extra_env: Variables added to the environment of the install, such as build flags.
        """
        _check(
            [
                "uv",
                "pip",
                "install",
                "--python",
                str(self.executable("python")),
                str(artifact),
            ],
            cwd=self.root,
            env={**self.env, **(extra_env or {})},
            timeout=SDIST_BUILD_TIMEOUT,
        )


def main() -> int:
    """Run every check selected on the command line."""
    options = _parse_args()
    _check_archive(options.wheel)
    for interpreter in options.python:
        with tempfile.TemporaryDirectory(prefix="omnicoach-wheel-") as temp_dir:
            _check_installed_wheel(
                root=pathlib.Path(temp_dir),
                interpreter=interpreter,
                wheel=options.wheel,
                reference_binary=options.reference_binary,
            )
    if options.sdist is not None:
        with tempfile.TemporaryDirectory(prefix="omnicoach-sdist-") as temp_dir:
            _check_sdist(
                root=pathlib.Path(temp_dir),
                interpreter=options.python[0],
                sdist=options.sdist,
            )
    print(
        f"Verified {options.wheel.name} ({options.wheel.stat().st_size} bytes) "
        f"across {len(options.python)} Python interpreter(s)."
    )
    return 0


def _parse_args() -> argparse.Namespace:
    """Parse flags and resolve the wheel, sdist, and reference binary paths."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--wheel-dir", type=pathlib.Path, required=True)
    parser.add_argument("--sdist-dir", type=pathlib.Path)
    parser.add_argument(
        "--python",
        action="append",
        help="Interpreter passed to `uv venv --python` (repeatable; default: current Python).",
    )
    parser.add_argument(
        "--reference-binary",
        help="Standalone omnicoach binary for output parity (`.exe` is resolved on Windows).",
    )
    args = parser.parse_args()
    args.wheel = _single_match(args.wheel_dir, "*.whl")
    args.sdist = _single_match(args.sdist_dir, "*.tar.gz") if args.sdist_dir else None
    args.python = args.python or [sys.executable]
    if args.reference_binary is not None:
        resolved = shutil.which(args.reference_binary)
        if resolved is None:
            parser.error(f"reference binary not found: {args.reference_binary}")
        args.reference_binary = pathlib.Path(resolved).resolve()
    return args


def _single_match(directory: pathlib.Path, pattern: str) -> pathlib.Path:
    """Return the only file matching `pattern` in `directory`."""
    matches = sorted(directory.glob(pattern))
    _expect(len(matches) == 1, f"exactly one {pattern} in {directory}, found {matches}")
    return matches[0].resolve()


def _check_archive(wheel: pathlib.Path) -> None:
    """Check the wheel tag, size, and that it ships a Python entry point rather than a binary."""
    _expect("-cp310-abi3-" in wheel.name, f"cp310-abi3 tag in {wheel.name}")
    _expect(
        wheel.stat().st_size <= MAX_WHEEL_BYTES,
        f"{wheel.name} under {MAX_WHEEL_BYTES} bytes",
    )
    with zipfile.ZipFile(wheel) as archive:
        entries = frozenset(archive.namelist())
    _expect(
        len(entries & EXTENSION_ENTRIES) == 1,
        f"one abi3 extension in {sorted(entries)}",
    )
    _expect(
        REQUIRED_ENTRIES <= entries, f"{sorted(REQUIRED_ENTRIES - entries)} missing"
    )
    scripts = [entry for entry in entries if ".data/scripts/" in entry]
    _expect(not scripts, f"no bundled executables, found {scripts}")


def _check_installed_wheel(
    *,
    root: pathlib.Path,
    interpreter: str,
    wheel: pathlib.Path,
    reference_binary: pathlib.Path | None,
) -> None:
    """Install `wheel` and run the CLI, parity, hook, and closed-pipe checks.

    Args:
        root: Empty scratch directory holding the environment and fixtures.
        interpreter: Python passed to `uv venv --python`.
        wheel: Wheel under test.
        reference_binary: Standalone binary whose output must match the wheel's, if given.
    """
    venv = Venv.create(root, interpreter)
    venv.install(wheel)
    fixtures = root / "fixtures"
    fixtures.mkdir()
    for name, content in FIXTURES:
        (fixtures / name).write_text(content, encoding="utf-8")
    omnicoach = [str(venv.executable("omnicoach"))]

    for case in CLI_CASES:
        outcome = _run([*omnicoach, *case.args], cwd=fixtures, env=venv.env)
        _expect(
            outcome.exit_code == case.exit_code,
            f"exit {case.exit_code} for {case}: {outcome}",
        )
        _expect(case.stdout_contains in outcome.stdout, f"stdout of {case}: {outcome}")
        if reference_binary is not None:
            native = _run(
                [str(reference_binary), *case.args], cwd=fixtures, env=venv.env
            )
            _expect(
                outcome == native,
                f"parity with {reference_binary} for {case}: {native}",
            )

    module = [str(venv.executable("python")), "-m", "omnicoach"]
    for args in MODULE_PARITY_ARGS:
        via_module = _run([*module, *args], cwd=fixtures, env=venv.env)
        via_script = _run([*omnicoach, *args], cwd=fixtures, env=venv.env)
        _expect(via_module == via_script, f"python -m parity for {args}: {via_module}")

    _check_antigravity_hooks(omnicoach=omnicoach, fixtures=fixtures, env=venv.env)
    for case in CLOSED_PIPE_CASES:
        outcome = _run_with_closed_stdout(
            [*omnicoach, *case.args], cwd=fixtures, env=venv.env
        )
        _expect(
            outcome == Outcome(case.exit_code, "", ""),
            f"closed pipe for {case}: {outcome}",
        )


def _check_antigravity_hooks(
    *, omnicoach: Iterable[str], fixtures: pathlib.Path, env: Mapping[str, str]
) -> None:
    """Check a clean command passes silently and a violating edit is injected on the next turn."""
    artifacts = fixtures / "artifacts"
    artifacts.mkdir()
    hook = [*omnicoach, "hook", "antigravity"]
    safe_call = {
        "name": "run_command",
        "args": {"CommandLine": SAFE_COMMAND, "Cwd": str(fixtures)},
    }
    safe = _run(
        [*hook, "pre-tool-use"],
        cwd=fixtures,
        env=env,
        stdin=json.dumps({"workspacePaths": [str(fixtures)], "toolCall": safe_call}),
    )
    _expect(safe == Outcome(0, "", ""), f"silent pass for a clean command: {safe}")

    edit_call = {
        "name": "write_to_file",
        "args": {
            "TargetFile": str(fixtures / "proposed.py"),
            "CodeContent": VIOLATING_PYTHON,
        },
    }
    edit_payload = {
        "workspacePaths": [str(fixtures)],
        "artifactDirectoryPath": str(artifacts),
        "toolCall": edit_call,
    }
    edit = _run(
        [*hook, "pre-tool-use"], cwd=fixtures, env=env, stdin=json.dumps(edit_payload)
    )
    _expect(edit.exit_code == 0 and not edit.stderr, f"violating edit accepted: {edit}")

    injection = _run(
        [*hook, "pre-invocation"],
        cwd=fixtures,
        env=env,
        stdin=json.dumps({"artifactDirectoryPath": str(artifacts)}),
    )
    _expect(injection.exit_code == 0, f"pre-invocation exit: {injection}")
    _expect(
        RULE in json.dumps(json.loads(injection.stdout)),
        f"{RULE} injected: {injection}",
    )


def _check_sdist(*, root: pathlib.Path, interpreter: str, sdist: pathlib.Path) -> None:
    """Build and install `sdist` with `--locked`, then lint a Rust file with the result."""
    venv = Venv.create(root, interpreter)
    venv.install(sdist, extra_env={"MATURIN_PEP517_ARGS": "--locked"})
    sample = root / "violating.rs"
    sample.write_text(VIOLATING_RUST, encoding="utf-8")
    outcome = _run(
        [str(venv.executable("omnicoach")), "code", "--select", RULE, str(sample)],
        cwd=root,
        env=venv.env,
    )
    _expect(
        outcome.exit_code == 1 and RULE in outcome.stdout,
        f"sdist build lints Rust: {outcome}",
    )


def _run(
    command: Sequence[str],
    *,
    cwd: pathlib.Path,
    env: Mapping[str, str],
    stdin: str | None = None,
) -> Outcome:
    """Run `command` to completion and capture its streams as text."""
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        input=stdin,
        stdin=None if stdin is not None else subprocess.DEVNULL,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=TIMEOUT.total_seconds(),
        check=False,
    )
    return Outcome(completed.returncode, completed.stdout, completed.stderr)


def _run_with_closed_stdout(
    command: Sequence[str], *, cwd: pathlib.Path, env: Mapping[str, str]
) -> Outcome:
    """Run `command` with stdout connected to a pipe whose read end is already closed.

    The first write then fails with `EPIPE` deterministically, independent of pipe buffer size.
    The returned `stdout` is always empty.
    """
    read_fd, write_fd = os.pipe()
    os.close(read_fd)
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=write_fd,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            timeout=TIMEOUT.total_seconds(),
            check=False,
        )
    finally:
        os.close(write_fd)
    return Outcome(completed.returncode, "", completed.stderr)


def _check(
    command: Sequence[str],
    *,
    cwd: pathlib.Path,
    env: Mapping[str, str],
    timeout: datetime.timedelta = TIMEOUT,
) -> None:
    """Run a setup `command`, raising with its output if it fails."""
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=timeout.total_seconds(),
        check=False,
    )
    output = f"{completed.stdout}\n{completed.stderr}"
    _expect(completed.returncode == 0, f"{command} to succeed, got: {output}")


def _expect(condition: bool, expectation: str) -> None:
    """Raise `RuntimeError` describing `expectation` when `condition` is false."""
    if not condition:
        raise RuntimeError(f"Expected {expectation}")


if __name__ == "__main__":
    sys.exit(main())
