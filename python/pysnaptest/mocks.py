"""Helpers for mocking functions while recording snapshot outputs.

These utilities make it easy to patch or wrap functions so their returned values
are automatically snapshot tested.
"""

from __future__ import annotations

from typing import Callable, Dict, Optional
import importlib
from unittest.mock import patch
import functools

from ._pysnaptest import mock_json_snapshot as _mock_json_snapshot, SnapshotInfo
from .assertion import extract_from_pytest_env


def mock_json_snapshot(
    func: Callable,
    record: bool = False,
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, str | int | None]] = None,
    allow_duplicates: bool = False,
):
    """Return a function mock that snapshots its JSON result.

    Args:
        func: Function to wrap with snapshot behaviour.
        record: Whether to record snapshots regardless of differences.
        snapshot_path: Optional path override for storing the snapshot.
        snapshot_name: Optional name override for the snapshot file.
        redactions: Mapping of selectors to replacement values.
        allow_duplicates: Whether to allow duplicate snapshot names.

    Returns:
        Callable: The wrapped function.
    """

    test_info = extract_from_pytest_env(snapshot_path, snapshot_name, allow_duplicates)
    return _mock_json_snapshot(func, test_info, record, redactions)


def resolve_function(dotted_path: str):
    """Resolve a dotted path to a callable.

    Args:
        dotted_path: ``module.attr`` style path to the target function.

    Returns:
        Callable: The resolved function object.
    """
    module_path, attr_name = dotted_path.rsplit(".", 1)
    module = importlib.import_module(module_path)
    return getattr(module, attr_name)


class patch_json_snapshot:
    """Patch a function so calls are snapshot tested.

    Instances of this class can be used as a context manager or decorator to
    temporarily replace a target function with a snapshotting mock.
    """

    def __init__(
        self,
        dotted_path: str,
        *,
        record: bool = False,
        snapshot_path: Optional[str] = None,
        snapshot_name: Optional[str] = None,
        redactions: Optional[Dict[str, str | int | None]] = None,
        allow_duplicates: bool = False,
    ):
        """Create the patch configuration.

        Args:
            dotted_path: ``module.attr`` style path to patch.
            record: Whether to always record new snapshots.
            snapshot_path: Optional path override for storing the snapshot.
            snapshot_name: Optional name override for the snapshot file.
            redactions: Mapping of selectors to replacement values.
            allow_duplicates: Whether to allow duplicate snapshot names.
        """

        self.dotted_path = dotted_path
        self.record = record
        self.snapshot_path = snapshot_path
        self.snapshot_name = snapshot_name
        self.redactions = redactions
        self.allow_duplicates = allow_duplicates
        self._patcher = None

    def __enter__(self):
        """Start patching and return the created mock.

        Returns:
            unittest.mock.MagicMock: The patched mock.
        """
        original_fn = resolve_function(self.dotted_path)
        mocked_fn = mock_json_snapshot(
            original_fn,
            record=self.record,
            snapshot_path=self.snapshot_path,
            snapshot_name=self.snapshot_name,
            redactions=self.redactions,
            allow_duplicates=self.allow_duplicates,
        )
        self._patcher = patch(self.dotted_path, side_effect=mocked_fn)
        self.mock = self._patcher.__enter__()
        return self.mock

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Stop patching and clean up."""

        return self._patcher.__exit__(exc_type, exc_val, exc_tb)

    def __call__(self, func: Callable):
        """Allow use of the object as a decorator.

        Args:
            func: The function being decorated.

        Returns:
            Callable: Wrapped function that applies the patch during execution.
        """

        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            with self:
                return func(*args, **kwargs)

        return wrapper
