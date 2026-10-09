"""Checks that each rule exemption has a dedicated test case that fails without the exemption.

Every `Mutation` (specs in `scripts/exemptions/`, one module per cluster of rules) disables one
exemption in a copy of the repository and runs the cases that exist to protect it. A case that
still passes tests nothing. The script also fails when a listed case no longer exists, when an
edit no longer matches the source, or when a `pass` case of a covered rule is not listed, so the
mapping cannot drift from the tests.

Usage (from the repository root):
    python3 scripts/exemption_mutations.py [--jobs N] [label substring]

Mutations run in parallel, each worker in its own copy of the repository and target directory
under `target/exemption-mutations/`, so the working tree is never modified. Re-run after any
change to a covered rule or to the extractor behind it.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import functools
import os
import pathlib
import queue
import re
import shutil
import subprocess
import sys
from collections.abc import Collection, Iterable, Mapping, Sequence

from exemptions import CLUSTERS
from exemptions.model import Mutation

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "target"
WORK_ROOT = ROOT / TARGET / "exemption-mutations"
COPY_IGNORED = (TARGET, "scratch", ".jj", ".git")
TEST_LISTING_SUFFIX = ": test"
FAILED = "FAILED"
DEFAULT_JOBS = max(1, min(4, (os.cpu_count() or 1) // 4))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("label_filter", nargs="?", default="", help="only run mutations whose label contains this")
    parser.add_argument("--jobs", type=int, default=DEFAULT_JOBS, help="parallel workers (default: %(default)s)")
    arguments = parser.parse_args()

    mutations = [
        mutation
        for cluster in CLUSTERS
        for mutation in cluster.mutations
        if re.search(arguments.label_filter, mutation.label)
        or any(re.search(arguments.label_filter, case) for case in mutation.cases)
    ]
    workers = prepare_workers(max(1, min(arguments.jobs, len(mutations))))
    test_names = list_tests(workers[0])
    problems = list(check_coverage(test_names))
    problems += run_mutations(mutations, test_names, workers)
    print("\nProblems:" if problems else "\nAll exemptions are protected.")
    for problem in problems:
        print(f"  {problem}")
    return 1 if problems else 0


def prepare_workers(count: int) -> tuple[pathlib.Path, ...]:
    """Refreshes `count` copies of the repository, keeping each copy's build cache."""
    workers = tuple(WORK_ROOT / f"worker-{index}" for index in range(count))
    for worker in workers:
        worker.mkdir(parents=True, exist_ok=True)
        for child in worker.iterdir():
            if child.name == TARGET:
                continue
            if child.is_dir():
                shutil.rmtree(child)
            else:
                child.unlink()
        for child in ROOT.iterdir():
            if child.name in COPY_IGNORED:
                continue
            if child.is_dir():
                shutil.copytree(child, worker / child.name, symlinks=True)
            else:
                shutil.copy2(child, worker / child.name)
    return workers


def list_tests(worker: pathlib.Path) -> tuple[str, ...]:
    listing = cargo_lib_tests(worker, "--list").stdout
    return tuple(
        line.removesuffix(TEST_LISTING_SUFFIX) for line in listing.splitlines() if line.endswith(TEST_LISTING_SUFFIX)
    )


def check_coverage(test_names: Sequence[str]) -> tuple[str, ...]:
    """Reports listed cases that do not exist and `pass` cases of covered rules that are not listed."""
    problems = []
    listed = set()
    covered_rules = {rule for cluster in CLUSTERS for rule in cluster.rules}
    for cluster in CLUSTERS:
        for entry in (*cluster.mutations, *cluster.unmutated):
            for case in entry.cases:
                matches = resolve(case, test_names)
                if not matches:
                    problems.append(f"{entry.label}: no test case `{case}`")
                listed.update(matches)
    for test_name in test_names:
        rule = re.search(r"rules::(?:\w+::tests_)?(\w+)(?:::tests)?::pass::", test_name)
        if rule and rule.group(1) in covered_rules and test_name not in listed:
            problems.append(f"unlisted pass case `{test_name}`: map it to an exemption in scripts/exemptions/")
    return tuple(problems)


def resolve(case: str, test_names: Iterable[str]) -> tuple[str, ...]:
    """Returns the generated test names of `case`, whose `case_N_` prefix shifts as cases are added."""
    case, _, language = case.partition("@")
    rule, kind, name = case.split("::")
    pattern = re.compile(rf"rules::(?:\w+::tests_)?{rule}(?:::tests)?::{kind}::case_\d+_{name}$")
    matches = tuple(sorted(test_name for test_name in test_names if pattern.search(test_name)))
    # `rule_test!` numbers Python cases before Rust cases.
    if language == "python":
        return matches[:1]
    if language == "rust":
        return matches[-1:]
    return matches


def run_mutations(
    mutations: Sequence[Mutation], test_names: Sequence[str], workers: Collection[pathlib.Path]
) -> tuple[str, ...]:
    """Runs each mutation on the next free worker; returns problems in spec order."""
    free_workers: queue.Queue[pathlib.Path] = queue.Queue()
    for worker in workers:
        free_workers.put(worker)
    run_one = functools.partial(run_on_free_worker, test_names=test_names, free_workers=free_workers)
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(workers)) as executor:
        results = executor.map(run_one, mutations)
        return tuple(problem for problems in results for problem in problems)


def run_on_free_worker(
    mutation: Mutation, test_names: Sequence[str], free_workers: queue.Queue[pathlib.Path]
) -> tuple[str, ...]:
    worker = free_workers.get()
    try:
        return run_mutation(mutation, test_names, worker)
    finally:
        free_workers.put(worker)


def run_mutation(mutation: Mutation, test_names: Sequence[str], worker: pathlib.Path) -> tuple[str, ...]:
    path = worker / mutation.path
    original = path.read_text()
    mutated = original
    for old, new in mutation.edits:
        if mutated.count(old) != 1:
            return (f"{mutation.label}: edit target found {mutated.count(old)} times in {mutation.path}: {old!r}",)
        mutated = mutated.replace(old, new, 1)
    cases = [test_name for case in mutation.cases for test_name in resolve(case, test_names)]
    path.write_text(mutated)
    try:
        outcomes = run_cases(worker, cases)
    finally:
        path.write_text(original)
    problems = []
    for test_name in cases:
        status = outcomes.get(test_name, "not run")
        print(f"{'killed' if status == FAILED else 'SURVIVED':9} {mutation.label}: {test_name}", flush=True)
        if status != FAILED:
            problems.append(f"{mutation.label}: `{test_name}` {status} without the exemption")
    return tuple(problems)


def run_cases(worker: pathlib.Path, test_names: Sequence[str]) -> Mapping[str, str]:
    result = cargo_lib_tests(worker, "--exact", *test_names)
    if "test result:" not in result.stdout:
        return dict.fromkeys(test_names, "did not build")
    return dict(re.findall(rf"^test (\S+) \.\.\. (ok|{FAILED})$", result.stdout, flags=re.MULTILINE))


def cargo_lib_tests(worker: pathlib.Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    """Runs library tests with lints capped, since a mutation often leaves a helper unused."""
    environment = {**os.environ, "RUSTFLAGS": "--cap-lints=warn", "CARGO_TARGET_DIR": str(worker / TARGET)}
    return subprocess.run(
        ["cargo", "test", "--lib", "--", *arguments], cwd=worker, env=environment, capture_output=True, text=True
    )


if __name__ == "__main__":
    sys.exit(main())
