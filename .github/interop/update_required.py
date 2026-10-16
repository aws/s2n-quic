#!/usr/bin/env python3

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

"""
Analyze interop results and update required.json with newly passing tests.

Downloads the interop reports for the last N commits on main, including every
retry attempt that was recorded, and identifies (test, impl, role) combinations
that passed in all of them (and were observed at least --min-samples times) but
are not yet in required.json, and removes required entries that failed more
than --max-failures times in the same window.
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import subprocess
import sys
import urllib.request
from collections import defaultdict
from typing import Any

CDN = "https://dnglbrstg7yg.cloudfront.net"
S2N_QUIC = "s2n-quic"

# The qns workflow retries failed runs on main up to this many times and
# uploads one result.json per attempt (logs/attempt-N/result.json). Reading
# every attempt means a tuple that failed on attempt 1 and passed on attempt 2
# is counted as 1 failure in 2 samples, rather than as a pass.
MAX_ATTEMPTS = 3

logger = logging.getLogger(__name__)


def _fetch(url: str, path: str) -> bool:
    """Download url to path. Returns False (and writes an empty marker file) on 404."""
    try:
        urllib.request.urlretrieve(url, path)
        return True
    except urllib.error.URLError:
        logger.debug("No interop results at %s", url)
        # Create empty file so we don't retry on subsequent runs
        with open(path, "w"):
            pass
        return False


def fetch_results(n_commits: int, out_dir: str) -> None:
    """Download interop results for the last n_commits on main.

    For each commit this fetches every per-attempt report that exists
    (logs/attempt-1 .. logs/attempt-MAX_ATTEMPTS) so retried runs contribute
    one sample per attempt. Commits that predate per-attempt uploads only have
    logs/latest, which is the final attempt; that is used as a single sample.
    """
    os.makedirs(out_dir, exist_ok=True)

    commits = subprocess.check_output(
        ["git", "log", "main", "-n", str(n_commits), "--format=format:%H"],
        text=True,
    ).strip().split("\n")

    for commit in commits:
        attempts_found = 0
        for attempt in range(1, MAX_ATTEMPTS + 1):
            path = os.path.join(out_dir, f"{commit}.attempt-{attempt}.json")
            if not os.path.exists(path):
                url = f"{CDN}/{commit}/interop/logs/attempt-{attempt}/result.json"
                if not _fetch(url, path):
                    break
            if os.path.getsize(path) == 0:
                break
            attempts_found += 1

        path = os.path.join(out_dir, f"{commit}.json")
        if attempts_found:
            # Don't double count a previously cached final result
            if os.path.exists(path):
                os.remove(path)
            continue

        if not os.path.exists(path):
            _fetch(f"{CDN}/{commit}/interop/logs/latest/result.json", path)


def collect_stats(
    results_dir: str,
) -> dict[tuple[str, str, str], list[int]]:
    """Parse all result files and collect pass/total stats per (test, impl, role).

    Returns a dict mapping (test, impl, role) to [passed, total], e.g.:
        {
            ("handshake", "quic-go", "client"): [48, 48],
            ("retry", "picoquic", "server"): [47, 48],
            ("http3", "s2n-quic", "client"): [48, 48],
        }
    """
    stats: dict[tuple[str, str, str], list[int]] = defaultdict(lambda: [0, 0])

    for filename in sorted(os.listdir(results_dir)):
        if not filename.endswith(".json"):
            continue
        filepath = os.path.join(results_dir, filename)
        if os.path.getsize(filepath) == 0:
            continue

        with open(filepath) as f:
            try:
                report = json.load(f)
            except json.JSONDecodeError:
                logger.debug("Skipping invalid JSON: %s", filename)
                continue

        clients = report.get("clients", [])
        servers = report.get("servers", [])
        results = report.get("results", [])

        # The results array is a flat list of test outcomes in row-major order
        # over (client, server) pairs. Each entry is an array of per-test results.
        # We only care about pairs where s2n-quic is either the client or server.
        idx = 0
        for client in clients:
            for server in servers:
                if idx >= len(results):
                    break
                result = results[idx]
                idx += 1

                if client != S2N_QUIC and server != S2N_QUIC:
                    continue

                for test in result:
                    name = test["name"]
                    success = test["result"] == "succeeded"

                    if client == S2N_QUIC and server == S2N_QUIC:
                        # Self-test: counts as both client and server passing
                        stats[(name, S2N_QUIC, "client")][1] += 1
                        stats[(name, S2N_QUIC, "server")][1] += 1
                        if success:
                            stats[(name, S2N_QUIC, "client")][0] += 1
                            stats[(name, S2N_QUIC, "server")][0] += 1
                    elif client == S2N_QUIC:
                        # s2n-quic client vs other server: record the other
                        # impl's success as a server
                        stats[(name, server, "server")][1] += 1
                        if success:
                            stats[(name, server, "server")][0] += 1
                    else:
                        # Other client vs s2n-quic server: record the other
                        # impl's success as a client
                        stats[(name, client, "client")][1] += 1
                        if success:
                            stats[(name, client, "client")][0] += 1

    return stats


def qualifies(stat: list[int] | None, min_samples: int) -> bool:
    """A tuple qualifies if it was observed at least min_samples times and never failed."""
    if stat is None:
        return False
    passed, total = stat
    return total >= min_samples and passed == total


def should_demote(stat: list[int] | None, max_failures: int) -> bool:
    """A required tuple is demoted once it has failed more than max_failures times."""
    if stat is None:
        return False
    passed, total = stat
    return total - passed > max_failures


def update_required(
    required_path: str,
    stats: dict[tuple[str, str, str], list[int]],
    min_samples: int,
    max_failures: int,
) -> tuple[dict[str, Any], list[tuple[str, str, str]], list[tuple[str, str, str]]]:
    """
    Update required.json: add entries that passed every one of at least
    min_samples observed runs, and remove entries that failed more than
    max_failures times.

    Returns (updated_json, additions, removals), each a list of
    (test, impl, role) tuples.
    """
    with open(required_path) as f:
        current = json.load(f)

    additions: list[tuple[str, str, str]] = []
    removals: list[tuple[str, str, str]] = []
    output: dict[str, Any] = {}

    # Process existing tests: preserve impl order, add new roles/impls
    for test in current:
        output[test] = {}

        # Preserve existing impls in order
        for impl in current[test]:
            existing = list(current[test][impl])
            for role in ["client", "server"]:
                if role in existing:
                    if should_demote(stats.get((test, impl, role)), max_failures):
                        existing.remove(role)
                        removals.append((test, impl, role))
                elif qualifies(stats.get((test, impl, role)), min_samples):
                    existing.append(role)
                    additions.append((test, impl, role))
            existing.sort()
            output[test][impl] = existing

        # Add new impls only if they have at least one passing role
        all_impls = {k[1] for k in stats if k[0] == test}
        for impl in sorted(all_impls - set(current[test].keys())):
            roles = []
            for role in ["client", "server"]:
                if qualifies(stats.get((test, impl, role)), min_samples):
                    roles.append(role)
                    additions.append((test, impl, role))
            if roles:
                output[test][impl] = roles

    # Add new tests not already in required.json
    all_tests = {k[0] for k in stats}
    known_impls = list(next(iter(current.values())).keys()) if current else []

    for test in sorted(all_tests - set(current.keys())):
        entry: dict[str, list[str]] = {}
        has_any = False

        # Known impls first to preserve familiar ordering
        for impl in known_impls:
            roles = []
            for role in ["client", "server"]:
                if qualifies(stats.get((test, impl, role)), min_samples):
                    roles.append(role)
                    additions.append((test, impl, role))
                    has_any = True
            entry[impl] = roles

        # New impls with at least one passing role
        all_impls = {k[1] for k in stats if k[0] == test}
        for impl in sorted(all_impls - set(known_impls)):
            roles = []
            for role in ["client", "server"]:
                if qualifies(stats.get((test, impl, role)), min_samples):
                    roles.append(role)
                    additions.append((test, impl, role))
                    has_any = True
            if roles:
                entry[impl] = roles

        if has_any:
            output[test] = entry

    return output, additions, removals


def format_pr_description(
    additions: list[tuple[str, str, str]],
    removals: list[tuple[str, str, str]],
    n_commits: int,
    min_samples: int,
    max_failures: int,
) -> str:
    """Generate a PR description from the lists of additions and removals."""
    if not additions and not removals:
        return ""

    def group(entries: list[tuple[str, str, str]]) -> list[str]:
        by_test: dict[str, list[tuple[str, str]]] = defaultdict(list)
        for test, impl, role in entries:
            by_test[test].append((impl, role))
        out = []
        for test in sorted(by_test.keys()):
            out.append(f"**{test}**:")
            for impl, role in sorted(by_test[test]):
                out.append(f"- {impl} ({role})")
            out.append("")
        return out

    lines = [
        "### Description of changes:",
        "",
        f"This PR updates the interop required checks based on every recorded "
        f"attempt of the interop runs for the last {n_commits} commits on main.",
        "",
    ]

    if additions:
        lines += [
            f"### New required checks (observed at least {min_samples} times, "
            f"never failed):",
            "",
            *group(additions),
        ]

    if removals:
        lines += [
            f"### Removed required checks (failed more than {max_failures} "
            f"time(s) in the window):",
            "",
            *group(removals),
        ]

    lines += [
        "### Testing:",
        "",
        f"Generated by `.github/interop/update_required.py` from the interop "
        f"results (including retry attempts) of the last {n_commits} commits "
        f"on main.",
    ]

    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--commits", type=int, default=100,
        help="Number of commits on main to analyze (default: 100)",
    )
    parser.add_argument(
        "--min-samples", type=int, default=None,
        help="Minimum number of observed runs before a tuple can be promoted "
        "(default: same as --commits)",
    )
    parser.add_argument(
        "--max-failures", type=int, default=1,
        help="A required tuple that failed more than this many times in the "
        "window is removed (default: 1)",
    )
    parser.add_argument(
        "--results-dir", default="target/interop/results",
        help="Directory to cache downloaded results",
    )
    parser.add_argument(
        "--required", default=".github/interop/required.json",
        help="Path to required.json",
    )
    parser.add_argument(
        "--dry-run", action="store_true",
        help="Print what would change without modifying files",
    )
    parser.add_argument(
        "--pr-description", action="store_true",
        help="Print PR description to stdout",
    )
    args = parser.parse_args()
    if args.min_samples is None:
        args.min_samples = args.commits

    logging.basicConfig(
        level=logging.DEBUG if os.environ.get("DEBUG") else logging.INFO,
    )

    fetch_results(args.commits, args.results_dir)
    stats = collect_stats(args.results_dir)

    if not stats:
        print("No interop results found", file=sys.stderr)
        sys.exit(0)

    updated, additions, removals = update_required(
        args.required, stats, args.min_samples, args.max_failures
    )

    if not additions and not removals:
        print("No changes to required.json", file=sys.stderr)
        sys.exit(0)

    print(
        f"Found {len(additions)} new entries to add and "
        f"{len(removals)} entries to remove",
        file=sys.stderr,
    )

    if args.pr_description:
        print(format_pr_description(
            additions, removals, args.commits, args.min_samples, args.max_failures
        ))

    if args.dry_run:
        for test, impl, role in sorted(additions):
            print(f"  + {test} / {impl} ({role})")
        for test, impl, role in sorted(removals):
            print(f"  - {test} / {impl} ({role})")
    else:
        with open(args.required, "w") as f:
            json.dump(updated, f, indent=2)
            f.write("\n")
        print(f"Updated {args.required}", file=sys.stderr)


if __name__ == "__main__":
    main()
