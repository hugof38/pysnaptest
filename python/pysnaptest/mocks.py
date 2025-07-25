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
    test_info = extract_from_pytest_env(snapshot_path, snapshot_name, allow_duplicates)
    return _mock_json_snapshot(func, test_info, record, redactions)


def resolve_function(dotted_path: str):
    """Given a dotted path, import the module and return the attribute."""
    module_path, attr_name = dotted_path.rsplit(".", 1)
    module = importlib.import_module(module_path)
    return getattr(module, attr_name)


class patch_json_snapshot:
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
        self.dotted_path = dotted_path
        self.record = record
        self.snapshot_path = snapshot_path
        self.snapshot_name = snapshot_name
        self.redactions = redactions
        self.allow_duplicates = allow_duplicates
        self._patcher = None

    def __enter__(self):
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
        return self._patcher.__exit__(exc_type, exc_val, exc_tb)

    def __call__(self, func: Callable):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            with self:
                return func(*args, **kwargs)

        return wrapper

