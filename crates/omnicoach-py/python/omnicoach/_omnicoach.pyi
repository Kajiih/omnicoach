"""Type stubs for the compiled `omnicoach._omnicoach` CPython ABI3 extension."""

from collections.abc import Sequence

def run_cli(args: Sequence[str]) -> int:
    """Execute the `omnicoach` CLI with `args` and return its exit code."""
