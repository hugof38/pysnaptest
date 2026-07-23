"""Cargo-free helpers to accept or reject pending snapshots.

When pytest is run with ``--snapshot-new`` (``INSTA_UPDATE=new``), insta writes
pending ``*@pysnap.snap.new`` files for changed snapshots and prints the diff at
assertion time. These helpers persist or discard those pending files through
insta's own ``Snapshot::save`` so the committed format stays correct.

For an interactive diff viewer, use ``cargo-insta review``; this module
deliberately does not re-implement diffing or a review UI.
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import List, Optional

from ._pysnaptest import (
    accept_pending_snapshot as _accept_pending_snapshot,
    reject_pending_snapshot as _reject_pending_snapshot,
    print_pending_diff as _print_pending_diff,
    SNAPSHOT_SUFFIX,
)

#: Glob that matches only pysnaptest's own committed snapshots. ``SNAPSHOT_SUFFIX``
#: (``@pysnap.snap``) is defined once in Rust (``src/common.rs``) and exported by
#: the compiled ``_pysnaptest`` module, so ``review`` and ``unused`` always agree
#: with the snapshot files insta actually writes.
SNAPSHOT_GLOB = f"**/*{SNAPSHOT_SUFFIX}"

#: Glob that matches only pysnaptest's own pending snapshots, so review never
#: touches pending files produced by other snapshot tools.
PENDING_GLOB = f"{SNAPSHOT_GLOB}.new"


def _root(root: Optional[str]) -> Path:
    return Path(root or os.environ.get("INSTA_WORKSPACE_ROOT") or ".")


def find_pending_snapshots(root: Optional[str] = None) -> List[Path]:
    """Find pending pysnaptest snapshots under ``root``.

    Args:
        root: Directory to search. Defaults to ``INSTA_WORKSPACE_ROOT`` if set,
            otherwise the current working directory.

    Returns:
        List[Path]: Sorted paths to ``*@pysnap.snap.new`` pending snapshots.
    """

    return sorted(_root(root).glob(PENDING_GLOB))


def accept_pending_snapshot(pending_path: str | Path) -> Path:
    """Accept a pending snapshot, persisting it to its ``.snap`` file.

    Args:
        pending_path: Path to a ``*@pysnap.snap.new`` file.

    Returns:
        Path: The target ``.snap`` path that was written.
    """

    return Path(_accept_pending_snapshot(str(pending_path)))


def reject_pending_snapshot(pending_path: str | Path) -> None:
    """Reject a pending snapshot, deleting its ``.snap.new`` file.

    Args:
        pending_path: Path to a ``*@pysnap.snap.new`` file.
    """

    _reject_pending_snapshot(str(pending_path))


def print_pending_diff(pending_path: str | Path, root: Optional[str] = None) -> None:
    """Print insta's own diff for a pending snapshot against its committed target.

    Args:
        pending_path: Path to a ``*@pysnap.snap.new`` file.
        root: Workspace root used to display relative paths. Defaults to
            ``INSTA_WORKSPACE_ROOT`` if set, otherwise the current directory.
    """

    _print_pending_diff(str(pending_path), str(_root(root)))


def accept_all(root: Optional[str] = None) -> List[Path]:
    """Accept every pending snapshot under ``root``.

    Args:
        root: Directory to search. See :func:`find_pending_snapshots`.

    Returns:
        List[Path]: The target ``.snap`` paths that were written.
    """

    return [accept_pending_snapshot(p) for p in find_pending_snapshots(root)]


def reject_all(root: Optional[str] = None) -> int:
    """Reject every pending snapshot under ``root``.

    Args:
        root: Directory to search. See :func:`find_pending_snapshots`.

    Returns:
        int: The number of pending snapshots rejected.
    """

    pending = find_pending_snapshots(root)
    for p in pending:
        reject_pending_snapshot(p)
    return len(pending)


def review(root: Optional[str] = None) -> int:
    """Interactively review pending snapshots, one at a time.

    Mirrors ``cargo insta review``: for each pending snapshot insta's own diff
    is shown, then you accept, reject, or skip it.

    Args:
        root: Directory to search. See :func:`find_pending_snapshots`.

    Returns:
        int: The number of pending snapshots that were accepted.
    """

    pending = find_pending_snapshots(root)
    if not pending:
        print("No pending snapshots.")
        return 0

    accepted = 0
    for index, path in enumerate(pending, start=1):
        print(f"\nReviewing [{index}/{len(pending)}]:")
        print_pending_diff(path, root)
        while True:
            choice = input("  [a]ccept  [r]eject  [s]kip  [q]uit? ").strip().lower()
            if choice in {"a", "accept"}:
                accept_pending_snapshot(path)
                accepted += 1
                break
            if choice in {"r", "reject"}:
                reject_pending_snapshot(path)
                break
            if choice in {"s", "skip", ""}:
                break
            if choice in {"q", "quit"}:
                return accepted
    return accepted
