#!/usr/bin/env python3

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

"""
Analyze interop results and update required.json with newly passing tests.

Downloads the last N interop reports from the CDN and identifies (test, impl,
role) combinations that pass 100% of the time but are not yet in required.json.
Only adds new entries; never removes existing ones.
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

logger = logging.getLogger(__name__)


def fetch_results(n_commits: int, out_dir: str) -> None:
    """Download interop results for the last n_commits on main."""
    os.makedirs(out_dir, exist_ok=True)

    commits = subprocess.check_output(
        ["git", "log", "main", "-n", str(n_commits), "--format=format:%H"],
        text=True,
    ).strip().split("\n")

    for commit in commits:
        path = os.path.join(out_dir, f"{commit}.json")
        if os.path.exists(path):
            continue
        url = f"{CDN}/{commit}/interop/logs/latest/result.json"
        try:
            urllib.request.urlretrieve(url, path)
        except urllib.error.URLError:
            logger.debug("No interop results for %s", commit)
            # Create empty file so we don't retry on subsequent runs
            with open(path, "w"):
                pass


def collect_stats(
    results_dir: str,
) -> dict[tuple[str, str, str], list[int]]:
    """Parse all result files and collect pass/total stats per (test, impl, role)."""
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


def update_required(
    required_path: str,
    stats: dict[tuple[str, str, str], list[int]],
) -> tuple[dict[str, Any], list[tuple[str, str, str]]]:
    """
    Update required.json with new 100% passing entries.

    Returns (updated_json, additions) where additions is a list of
    (test, impl, role) tuples that were added.
    """
    with open(required_path) as f:
        current = json.load(f)

    additions: list[tuple[str, str, str]] = []
    output: dict[str, Any] = {}

    # Process existing tests: preserve impl order, add new roles/impls
    for test in current:
        output[test] = {}

        # Preserve existing impls in order
        for impl in current[test]:
            existing = list(current[test][impl])
            for role in ["client", "server"]:
                if role not in existing:
                    passed, total = stats.get((test, impl, role), [0, 0])
                    if total > 0 and passed == total:
                        existing.append(role)
                        additions.append((test, impl, role))
            existing.sort()
            output[test][impl] = existing

        # Add new impls only if they have at least one passing role
        all_impls = {k[1] for k in stats if k[0] == test}
        for impl in sorted(all_impls - set(current[test].keys())):
            roles = []
            for role in ["client", "server"]:
                passed, total = stats.get((test, impl, role), [0, 0])
                if total > 0 and passed == total:
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
                passed, total = stats.get((test, impl, role), [0, 0])
                if total > 0 and passed == total:
                    roles.append(role)
                    additions.append((test, impl, role))
                    has_any = True
            entry[impl] = roles

        # New impls with at least one passing role
        all_impls = {k[1] for k in stats if k[0] == test}
        for impl in sorted(all_impls - set(known_impls)):
            roles = []
            for role in ["client", "server"]:
                passed, total = stats.get((test, impl, role), [0, 0])
                if total > 0 and passed == total:
                    roles.append(role)
                    additions.append((test, impl, role))
                    has_any = True
            if roles:
                entry[impl] = roles

        if has_any:
            output[test] = entry

    return output, additions


def format_pr_description(
    additions: list[tuple[str, str, str]],
    n_commits: int,
) -> str:
    """Generate a PR description from the list of additions."""
    if not additions:
        return ""

    by_test: dict[str, list[tuple[str, str]]] = defaultdict(list)
    for test, impl, role in additions:
        by_test[test].append((impl, role))

    lines = [
        "### Description of changes:",
        "",
        f"This PR updates the interop required checks based on the last "
        f"{n_commits} interop reports. All additions have a 100% pass rate "
        f"across all analyzed reports.",
        "",
        "### New required checks:",
        "",
    ]

    for test in sorted(by_test.keys()):
        entries = sorted(by_test[test])
        lines.append(f"**{test}**:")
        for impl, role in entries:
            lines.append(f"- {impl} ({role})")
        lines.append("")

    lines.extend([
        "### Testing:",
        "",
        f"All entries added have passed 100% of the interop runs in the "
        f"last {n_commits} reports on main. No existing entries were removed.",
    ])

    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--commits", type=int, default=50,
        help="Number of commits to analyze (default: 50)",
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

    logging.basicConfig(
        level=logging.DEBUG if os.environ.get("DEBUG") else logging.INFO,
    )

    fetch_results(args.commits, args.results_dir)
    stats = collect_stats(args.results_dir)

    if not stats:
        print("No interop results found", file=sys.stderr)
        sys.exit(0)

    updated, additions = update_required(args.required, stats)

    if not additions:
        print("No new entries to add", file=sys.stderr)
        sys.exit(0)

    print(f"Found {len(additions)} new entries to add", file=sys.stderr)

    if args.pr_description:
        print(format_pr_description(additions, args.commits))

    if args.dry_run:
        for test, impl, role in sorted(additions):
            print(f"  {test} / {impl} ({role})")
    else:
        with open(args.required, "w") as f:
            json.dump(updated, f, indent=2)
            f.write("\n")
        print(f"Updated {args.required}", file=sys.stderr)


if __name__ == "__main__":
    main()
