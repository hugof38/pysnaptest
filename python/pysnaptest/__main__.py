"""Command-line entry point for reviewing pending snapshots.

Run ``pysnaptest --help`` for usage. Mirrors the common
``cargo insta`` subcommands (``review``, ``accept``, ``reject``,
``pending-snapshots``, ``unused``) but works without any Rust tooling.
"""

from __future__ import annotations

import argparse
from typing import Optional, Sequence

from .review import (
    accept_all,
    find_pending_snapshots,
    print_pending_diff,
    reject_all,
    review,
)
from .unused import delete_snapshot, unused_snapshots


def build_parser() -> argparse.ArgumentParser:
    """Build the argument parser for the ``pysnaptest`` CLI."""

    parser = argparse.ArgumentParser(
        prog="pysnaptest",
        description="Review, accept, or reject pending snapshots without cargo-insta.",
    )
    parser.add_argument(
        "--root",
        default=None,
        help="Directory to search for pending snapshots "
        "(defaults to $INSTA_WORKSPACE_ROOT or the current directory).",
    )
    sub = parser.add_subparsers(dest="command")
    sub.add_parser("review", help="Interactively review each pending snapshot.")
    sub.add_parser("accept", help="Accept all pending snapshots.")
    sub.add_parser("reject", help="Reject all pending snapshots.")
    sub.add_parser("pending", help="List pending snapshots and their diffs.")
    unused = sub.add_parser(
        "unused",
        help="Run the test suite and report snapshots no test referenced.",
    )
    unused.add_argument(
        "--delete",
        action="store_true",
        help="Delete the unreferenced snapshots (and any binary sidecars).",
    )
    unused.add_argument(
        "pytest_args",
        nargs=argparse.REMAINDER,
        help="Arguments forwarded to pytest (e.g. a test path). "
        "Prefix with -- to separate them from pysnaptest options.",
    )

    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    """Run the CLI.

    Args:
        argv: Optional argument list (defaults to ``sys.argv``).

    Returns:
        int: Process exit code.
    """

    args = build_parser().parse_args(argv)

    if args.command == "accept":
        written = accept_all(args.root)
        print(f"Accepted {len(written)} snapshot(s).")
    elif args.command == "reject":
        count = reject_all(args.root)
        print(f"Rejected {count} snapshot(s).")
    elif args.command == "pending":
        pending = find_pending_snapshots(args.root)
        for path in pending:
            print(f"\n{path}")
            print_pending_diff(path, args.root)
        print(f"\n{len(pending)} pending snapshot(s).")
    elif args.command == "unused":
        return _unused_command(args)
    else:  # "review" or no subcommand
        review(args.root)
    return 0


def _unused_command(args: argparse.Namespace) -> int:
    """Handle ``pysnaptest unused``: run the suite, then report or delete.

    Args:
        args: Parsed CLI arguments.

    Returns:
        int: ``0`` when nothing was left unreferenced, ``1`` otherwise so the
        command can gate CI when run without ``--delete``.
    """

    pytest_args = [a for a in args.pytest_args if a != "--"]
    unused = unused_snapshots(args.root, pytest_args)

    if not unused:
        print("No unused snapshots found.")
        return 0

    if args.delete:
        print(f"Deleting {len(unused)} unused snapshot(s):")
        for path in unused:
            delete_snapshot(path)
            print(f"  {path}")
        return 0

    print(f"Found {len(unused)} unused snapshot(s):")
    for path in unused:
        print(f"  {path}")
    print("Re-run with `pysnaptest unused --delete` to remove them.")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
