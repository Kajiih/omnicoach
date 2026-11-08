"""Console-script and `python -m omnicoach` entrypoint delegating to `_omnicoach`."""

import sys
from typing import NoReturn

from omnicoach._omnicoach import run_cli

SIGINT_EXIT_CODE = 130
CLI_PROGRAM_NAME = "omnicoach"


def main() -> NoReturn:
    """Run the `omnicoach` CLI using `sys.argv` and raise `SystemExit` with its exit code.

    The Rust CLI writes straight to file descriptors 1 and 2 and handles a closed stdout pipe
    itself, so only `Ctrl-C` needs translating here.
    """
    try:
        exit_code = run_cli([CLI_PROGRAM_NAME, *sys.argv[1:]])
    except KeyboardInterrupt:
        raise SystemExit(SIGINT_EXIT_CODE) from None
    raise SystemExit(exit_code)
