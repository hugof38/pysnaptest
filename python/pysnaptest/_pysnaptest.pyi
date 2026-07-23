"""Type stubs for the compiled ``pysnaptest._pysnaptest`` extension module.

These declarations mirror the pyo3 bindings defined in ``src/`` so that editors
and type checkers can offer completion and validation for the Rust-backed API.
"""

import os
from pathlib import Path
from typing import Any, Optional, Union

_StrPath = Union[str, os.PathLike[str]]
_Redactions = dict[str, Union[str, int, None]]

class SnapshotInfo:
    """Snapshot configuration resolved from the active pytest test."""

    @staticmethod
    def from_pytest(
        snapshot_path_override: Optional[_StrPath] = ...,
        snapshot_name_override: Optional[str] = ...,
        allow_duplicates: bool = ...,
    ) -> "SnapshotInfo":
        """Build snapshot info from the ``PYTEST_CURRENT_TEST`` environment."""
        ...

    def snapshot_folder(self) -> Path:
        """Return the directory snapshots are stored in."""
        ...

    def last_snapshot_name(self) -> str:
        """Return the name of the most recently used snapshot."""
        ...

    def next_snapshot_name(self) -> str:
        """Return the name the next snapshot assertion will use."""
        ...

    def last_snapshot_path(self, module_path: Optional[str]) -> Path:
        """Return the path of the most recently used snapshot."""
        ...

    def next_snapshot_path(self, module_path: Optional[str]) -> Path:
        """Return the path the next snapshot assertion will write."""
        ...

class PySnapshot:
    """A snapshot loaded from disk via insta."""

    @staticmethod
    def from_file(p: _StrPath) -> "PySnapshot":
        """Load a snapshot from ``p``."""
        ...

    def contents(self) -> bytes:
        """Return the snapshot contents as raw bytes."""
        ...

def assert_json_snapshot(
    test_info: SnapshotInfo,
    result: Any,
    redactions: Optional[_Redactions] = ...,
) -> None:
    """Assert that ``result`` matches its stored JSON snapshot."""
    ...

def assert_csv_snapshot(
    test_info: SnapshotInfo,
    result: Any,
    redactions: Optional[_Redactions] = ...,
) -> None:
    """Assert that CSV text matches its stored snapshot."""
    ...

def assert_binary_snapshot(
    test_info: SnapshotInfo,
    extension: str,
    result: bytes,
) -> None:
    """Assert that binary data matches its stored snapshot."""
    ...

def assert_snapshot(test_info: SnapshotInfo, result: Any) -> None:
    """Assert that a value matches its stored text snapshot."""
    ...

def assert_json_snapshot_named(
    test_info: SnapshotInfo,
    result: Any,
    name: str,
    redactions: Optional[_Redactions] = ...,
) -> None:
    """Assert a JSON snapshot under an explicit ``name`` (no counter tick)."""
    ...

def prepare_mock_call(
    test_info: SnapshotInfo,
    suffix: str,
    request: Any,
    record: bool,
    redactions: Optional[_Redactions] = ...,
) -> tuple[str, Path, bool]:
    """Scope ``test_info`` to a mock, write its request snapshot, and return
    ``(name, response_path, do_record)`` for the response."""
    ...

def read_json_snapshot(snapshot_path: _StrPath) -> Any:
    """Load a recorded JSON snapshot file and return its parsed value."""
    ...

def accept_pending_snapshot(pending_path: _StrPath) -> Path:
    """Accept a pending snapshot, persisting it to its ``.snap`` file."""
    ...

def reject_pending_snapshot(pending_path: _StrPath) -> None:
    """Reject a pending snapshot, deleting its ``.snap.new`` file."""
    ...

def print_pending_diff(
    pending_path: _StrPath,
    workspace_root: Optional[_StrPath] = ...,
) -> None:
    """Print insta's own diff for a pending snapshot against its target."""
    ...
