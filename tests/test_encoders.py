"""Tests for automatic JSON-encoding of rich Python objects in snapshots."""

from __future__ import annotations

import dataclasses
from datetime import date, datetime, time, timedelta
from decimal import Decimal
from enum import Enum
from pathlib import PurePosixPath
from uuid import UUID

import pytest

from pysnaptest import (
    assert_json_snapshot,
    is_jsonable_object,
    snapshot,
    to_jsonable,
)


class Color(Enum):
    RED = "red"
    GREEN = "green"


@dataclasses.dataclass
class Point:
    x: int
    y: int


@dataclasses.dataclass
class Line:
    start: Point
    end: Point
    tags: set


# ---------------------------------------------------------------------------
# to_jsonable unit tests (fast, no snapshot files)
# ---------------------------------------------------------------------------


def test_to_jsonable_passes_through_native_scalars():
    assert to_jsonable(1) == 1
    assert to_jsonable(1.5) == 1.5
    assert to_jsonable("x") == "x"
    assert to_jsonable(True) is True
    assert to_jsonable(None) is None


def test_to_jsonable_enum_uses_value():
    assert to_jsonable(Color.RED) == "red"


def test_to_jsonable_datetime_family_uses_isoformat():
    assert to_jsonable(datetime(2020, 1, 2, 3, 4, 5)) == "2020-01-02T03:04:05"
    assert to_jsonable(date(2020, 1, 2)) == "2020-01-02"
    assert to_jsonable(time(3, 4, 5)) == "03:04:05"


def test_to_jsonable_timedelta_uses_total_seconds():
    assert to_jsonable(timedelta(seconds=90)) == 90.0


def test_to_jsonable_uuid_and_path_use_str():
    assert to_jsonable(UUID(int=0)) == "00000000-0000-0000-0000-000000000000"
    assert to_jsonable(PurePosixPath("/a/b")) == "/a/b"


def test_to_jsonable_decimal_int_and_float():
    assert to_jsonable(Decimal("5")) == 5
    assert to_jsonable(Decimal("1.5")) == 1.5


def test_to_jsonable_bytes_decode_and_hex_fallback():
    assert to_jsonable(b"hello") == "hello"
    assert to_jsonable(b"\xff\xfe") == "fffe"


def test_to_jsonable_set_is_sorted_when_orderable():
    assert to_jsonable({3, 1, 2}) == [1, 2, 3]


def test_to_jsonable_tuple_becomes_list():
    assert to_jsonable((1, 2, 3)) == [1, 2, 3]


def test_to_jsonable_mapping_stringifies_non_str_keys():
    assert to_jsonable({1: "a", "b": 2}) == {"1": "a", "b": 2}


def test_to_jsonable_dataclass_recurses():
    line = Line(start=Point(0, 0), end=Point(1, 1), tags={"b", "a"})
    assert to_jsonable(line) == {
        "start": {"x": 0, "y": 0},
        "end": {"x": 1, "y": 1},
        "tags": ["a", "b"],
    }


def test_to_jsonable_custom_encoder_takes_priority():
    assert to_jsonable(Point(1, 2), custom_encoder={Point: lambda p: [p.x, p.y]}) == [
        1,
        2,
    ]


def test_to_jsonable_handles_reference_cycles():
    a: dict = {}
    a["self"] = a
    assert to_jsonable(a) == {"self": "<circular reference>"}


def test_to_jsonable_unknown_object_falls_back_to_str():
    class Widget:
        def __str__(self) -> str:
            return "widget-repr"

    assert to_jsonable(Widget()) == "widget-repr"


def test_is_jsonable_object_predicate():
    assert is_jsonable_object(Point(1, 2)) is True
    assert is_jsonable_object(Color.RED) is True
    assert is_jsonable_object({1, 2}) is True
    assert is_jsonable_object((1, 2)) is True
    assert is_jsonable_object("x") is False
    assert is_jsonable_object(b"x") is False
    assert is_jsonable_object(5) is False


# ---------------------------------------------------------------------------
# Snapshot integration tests
# ---------------------------------------------------------------------------


def test_assert_json_snapshot_dataclass():
    assert_json_snapshot(Line(start=Point(0, 0), end=Point(2, 3), tags={"b", "a"}))


def test_assert_json_snapshot_enum():
    assert_json_snapshot({"color": Color.GREEN, "when": datetime(2021, 6, 1, 12)})


def test_assert_json_snapshot_custom_encoder():
    assert_json_snapshot(
        {"point": Point(4, 5)},
        custom_encoder={Point: lambda p: {"px": p.x, "py": p.y}},
    )


def test_assert_json_snapshot_rejects_dataframe():
    pd = pytest.importorskip("pandas")
    df = pd.DataFrame({"a": [1, 2]})
    with pytest.raises(TypeError, match="assert_dataframe_snapshot"):
        assert_json_snapshot(df)


@snapshot
def test_snapshot_decorator_dataclass() -> Point:
    return Point(7, 8)


# ---------------------------------------------------------------------------
# Pydantic-specific tests (optional dependency)
# ---------------------------------------------------------------------------

pydantic = pytest.importorskip("pydantic")


class Address(pydantic.BaseModel):
    street: str
    zipcode: str


class User(pydantic.BaseModel):
    name: str
    created_at: datetime
    user_id: UUID
    balance: Decimal
    address: Address
    role: Color


def _make_user() -> "User":
    return User(
        name="Ada",
        created_at=datetime(2020, 1, 1, 0, 0, 0),
        user_id=UUID(int=1),
        balance=Decimal("10.50"),
        address=Address(street="1 Main St", zipcode="00000"),
        role=Color.GREEN,
    )


def test_to_jsonable_pydantic_model():
    assert to_jsonable(_make_user()) == {
        "name": "Ada",
        "created_at": "2020-01-01T00:00:00",
        "user_id": "00000000-0000-0000-0000-000000000001",
        "balance": "10.50",
        "address": {"street": "1 Main St", "zipcode": "00000"},
        "role": "green",
    }


def test_assert_json_snapshot_pydantic_model():
    assert_json_snapshot(_make_user())


def test_assert_json_snapshot_pydantic_list():
    assert_json_snapshot([_make_user(), _make_user()])


@snapshot
def test_snapshot_decorator_pydantic_model() -> "User":
    return _make_user()


def test_assert_json_snapshot_pydantic_with_redactions():
    assert_json_snapshot(
        _make_user(),
        redactions={".user_id": "[uuid]", ".created_at": "[ts]"},
    )


class Event(pydantic.BaseModel):
    name: str
    happened_on: date


@snapshot
def test_snapshot_list_of_models_with_date() -> "list[Event]":
    return [
        Event(name="launch", happened_on=date(2026, 1, 15)),
        Event(name="review", happened_on=date(2026, 3, 2)),
    ]
