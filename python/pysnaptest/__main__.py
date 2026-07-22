"""Command-line entry point for reviewing pending snapshots.

Run ``python -m pysnaptest --help`` for usage. Mirrors the common
``cargo insta`` subcommands (``review``, ``accept``, ``reject``,
``pending-snapshots``) but works without any Rust tooling.
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
    else:  # "review" or no subcommand
        review(args.root)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

