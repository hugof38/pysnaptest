"""Convert arbitrary Python objects into JSON-native structures.

The snapshot machinery serializes values on the Rust side using
``pythonize::depythonize``, which only understands native Python types
(``dict``, ``list``, ``str``, ``int``, ``float``, ``bool`` and ``None``). This
module provides :func:`to_jsonable`, a dependency-free normalizer -- inspired by
FastAPI's ``jsonable_encoder`` -- that turns Pydantic models, dataclasses,
enums and common standard-library types into those native structures before
they cross the boundary.

Detection avoids importing ``pydantic`` at module load time, so it remains an
optional, unpinned dependency: the import is performed lazily inside the
relevant helpers and guarded against ``ImportError``.
"""

from __future__ import annotations

import dataclasses
from collections.abc import Mapping
from datetime import date, datetime, time, timedelta
from decimal import Decimal
from enum import Enum
from pathlib import PurePath
from typing import Any, Callable, Dict, Optional, Set
from uuid import UUID


def is_pydantic(obj: Any) -> bool:
    """Check whether ``obj`` is a Pydantic ``BaseModel`` instance.

    Works with both Pydantic v1 and v2: ``pydantic.BaseModel`` always refers to
    the installed version's base class.

    Args:
        obj: Object to test.

    Returns:
        bool: ``True`` if ``obj`` is a Pydantic model instance.
    """

    try:
        import pydantic
    except ImportError:
        return False

    return isinstance(obj, pydantic.BaseModel)


def _pydantic_to_dict(obj: Any) -> Any:
    """Dump a Pydantic model to a JSON-native ``dict``.

    Uses ``model_dump(mode="json")`` for v2 models and ``.dict()`` for v1 models.

    Args:
        obj: A Pydantic model instance.

    Returns:
        Any: The model's fields as a ``dict``.
    """

    if hasattr(obj, "model_dump"):
        return obj.model_dump(mode="json")
    return obj.dict()


def _is_dataclass_instance(obj: Any) -> bool:
    """Return ``True`` if ``obj`` is a dataclass instance (not the class)."""

    return dataclasses.is_dataclass(obj) and not isinstance(obj, type)


def is_jsonable_object(obj: Any) -> bool:
    """Report whether ``obj`` should be routed to a JSON snapshot.

    This is used by the snapshot dispatcher to decide whether an object that is
    not already a native ``dict``/``list`` should nonetheless be serialized as
    JSON (via :func:`to_jsonable`) rather than snapshotted as a string.

    Args:
        obj: Object to test.

    Returns:
        bool: ``True`` for Pydantic models, dataclasses, enums, sets, tuples and
        mappings.
    """

    if isinstance(obj, (str, bytes, bytearray)):
        return False
    return (
        is_pydantic(obj)
        or _is_dataclass_instance(obj)
        or isinstance(obj, (Enum, set, frozenset, tuple, Mapping))
    )


def _decimal_to_jsonable(value: Decimal) -> Any:
    """Convert a ``Decimal`` to ``int`` or ``float`` like FastAPI does."""

    if value.as_tuple().exponent >= 0:  # type: ignore[operator]
        return int(value)
    return float(value)


def to_jsonable(
    obj: Any,
    *,
    custom_encoder: Optional[Dict[type, Callable[[Any], Any]]] = None,
    _seen: Optional[Set[int]] = None,
) -> Any:
    """Recursively convert ``obj`` into JSON-native Python structures.

    The returned value is composed only of ``dict``, ``list``, ``str``,
    ``int``, ``float``, ``bool`` and ``None`` and is therefore safe to hand to
    the Rust snapshot serializer.

    Resolution order (first match wins): ``custom_encoder`` overrides, native
    scalars, Pydantic v2/v1 models, dataclasses, enums, ``datetime`` family,
    ``UUID``/``PurePath``/``Decimal``, ``bytes``, mappings, and
    set/tuple/list-style containers. Anything else falls back to ``str(obj)``.

    Args:
        obj: Object to convert.
        custom_encoder: Optional mapping of types to encoder callables, matching
            the ``custom_encoder`` argument of FastAPI's ``jsonable_encoder``.
            The first entry whose type matches ``obj`` (via ``isinstance``) wins.
        _seen: Internal set of ``id()`` values used to guard against reference
            cycles. Callers should not pass this.

    Returns:
        Any: A JSON-native representation of ``obj``.
    """

    if custom_encoder:
        for encoder_type, encoder in custom_encoder.items():
            if isinstance(obj, encoder_type):
                return to_jsonable(
                    encoder(obj), custom_encoder=custom_encoder, _seen=_seen
                )

    # Native scalars pass straight through. ``bool`` is a subclass of ``int``
    # and is handled here as well.
    if obj is None or isinstance(obj, (str, int, float, bool)):
        return obj

    if _seen is None:
        _seen = set()

    def recurse(value: Any) -> Any:
        return to_jsonable(value, custom_encoder=custom_encoder, _seen=_seen)

    if is_pydantic(obj):
        return recurse(_pydantic_to_dict(obj))

    if _is_dataclass_instance(obj):
        return recurse(dataclasses.asdict(obj))

    if isinstance(obj, Enum):
        return recurse(obj.value)

    if isinstance(obj, (datetime, date, time)):
        return obj.isoformat()

    if isinstance(obj, timedelta):
        return obj.total_seconds()

    if isinstance(obj, (UUID, PurePath)):
        return str(obj)

    if isinstance(obj, Decimal):
        return _decimal_to_jsonable(obj)

    if isinstance(obj, (bytes, bytearray)):
        try:
            return bytes(obj).decode()
        except UnicodeDecodeError:
            return bytes(obj).hex()

    obj_id = id(obj)
    if obj_id in _seen:
        return "<circular reference>"

    if isinstance(obj, Mapping):
        _seen.add(obj_id)
        try:
            result: Dict[str, Any] = {}
            for key, value in obj.items():
                encoded_key = recurse(key)
                if not isinstance(encoded_key, str):
                    encoded_key = str(encoded_key)
                result[encoded_key] = recurse(value)
            return result
        finally:
            _seen.discard(obj_id)

    if isinstance(obj, (set, frozenset)):
        _seen.add(obj_id)
        try:
            try:
                items = sorted(obj)
            except TypeError:
                items = list(obj)
            return [recurse(item) for item in items]
        finally:
            _seen.discard(obj_id)

    if isinstance(obj, (list, tuple)):
        _seen.add(obj_id)
        try:
            return [recurse(item) for item in obj]
        finally:
            _seen.discard(obj_id)

    return str(obj)
