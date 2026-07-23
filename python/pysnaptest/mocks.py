"""Helpers for mocking functions while recording snapshot outputs.

These utilities make it easy to patch or wrap functions so their returned
values are automatically snapshot tested.

Deciding whether to actually call the wrapped function, and normalizing rich
return values (Pydantic models, dataclasses, ...) via `to_jsonable`, happens
here in Python. But the fiddly snapshot-naming bookkeeping -- scoping a mock's
name, writing its request snapshot, and peeking its response path *before*
ticking the shared duplicate counter -- is owned by the Rust
`prepare_mock_call` primitive, which composes the same `SnapshotInfo` naming
methods used by the regular JSON snapshot machinery. The remaining two Rust
primitives, `assert_json_snapshot_named` and `read_json_snapshot`, write and
read the response snapshot once Python has decided what belongs there.
"""

from __future__ import annotations

import functools
import importlib
import inspect
from typing import Callable, Dict, Optional, Union
from unittest.mock import patch

from ._pysnaptest import (
    assert_json_snapshot_named as _assert_json_snapshot_named,
    prepare_mock_call as _prepare_mock_call,
    read_json_snapshot as _read_json_snapshot,
)
from .assertion import extract_from_pytest_env
from .encoders import to_jsonable


def mock_json_snapshot(
    func: Callable,
    record: bool = False,
    snapshot_path: Optional[str] = None,
    snapshot_name: Optional[str] = None,
    redactions: Optional[Dict[str, Union[str, int, None]]] = None,
    allow_duplicates: bool = False,
):
    """Return a function mock that snapshots its JSON result.

    Both synchronous and async def functions are supported: an async func
    yields an async mock that awaits the real function while recording.

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
    suffix = getattr(func, "__name__", "mocked_fn")

    if inspect.iscoroutinefunction(func):

        @functools.wraps(func)
        async def async_wrapper(*args, **kwargs):
            request = to_jsonable({"args": list(args), "kwargs": kwargs or None})
            name, response_path, do_record = _prepare_mock_call(
                test_info, suffix, request, record, redactions
            )
            if do_record:
                result = await func(*args, **kwargs)
                _assert_json_snapshot_named(
                    test_info, to_jsonable(result), name, redactions
                )
                return result
            return _read_json_snapshot(response_path)

        return async_wrapper

    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        request = to_jsonable({"args": list(args), "kwargs": kwargs or None})
        name, response_path, do_record = _prepare_mock_call(
            test_info, suffix, request, record, redactions
        )
        if do_record:
            result = func(*args, **kwargs)
            _assert_json_snapshot_named(
                test_info, to_jsonable(result), name, redactions
            )
            return result
        return _read_json_snapshot(response_path)

    return wrapper


def resolve_function(dotted_path: str):
    """Resolve a dotted path to a callable.

    Args:
        dotted_path: module.attr style path to the target function.

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
        redactions: Optional[Dict[str, Union[str, int, None]]] = None,
        allow_duplicates: bool = False,
    ):
        """Create the patch configuration.

        Args:
            dotted_path: module.attr style path to patch.
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

        if inspect.iscoroutinefunction(func):

            @functools.wraps(func)
            async def async_wrapper(*args, **kwargs):
                with self:
                    return await func(*args, **kwargs)

            return async_wrapper

        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            with self:
                return func(*args, **kwargs)

        return wrapper
