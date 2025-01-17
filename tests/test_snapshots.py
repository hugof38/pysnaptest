from __future__ import annotations

from pysnaptest import (
    snapshot,
    assert_json_snapshot,
    assert_dataframe_snapshot,
)
import pytest

try:
    import pandas as pd

    PANDAS_UNAVAILABLE = False
except ImportError:
    PANDAS_UNAVAILABLE = True

try:
    import polars as pl

    POLARS_UNAVAILABLE = False
except ImportError:
    POLARS_UNAVAILABLE = True


@snapshot
def test_snapshot_number() -> int:
    return 5


@snapshot
def test_snapshot_dict_result() -> dict[str, str]:
    return {"test": 2}


@snapshot
def test_snapshot_list_result() -> list[str]:
    return [1, 2, 4]


def test_assert_json_snapshot():
    assert_json_snapshot({"assert_json_snapshot": "expected_result"})


def test_assert_snapshot():
    assert_json_snapshot("expected_result")


@pytest.mark.skipif(PANDAS_UNAVAILABLE, reason="Pandas is an optional dependency")
def test_assert_pandas_dataframe_snapshot():
    df = pd.DataFrame({"name": ["foo", "bar"], "id": [1, 2]})
    assert_dataframe_snapshot(df, index=False)


@pytest.mark.skipif(POLARS_UNAVAILABLE, reason="Polars is an optional dependency")
@snapshot
def test_assert_polars_dataframe_snapshot() -> pl.DataFrame:
    return pl.DataFrame(
        {
            "foo": [1, 2, 3, 4, 5],
            "bar": [6, 7, 8, 9, 10],
            "ham": ["a", "b", "c", "d", "e"],
        }
    )


@pytest.mark.asyncio
@snapshot
async def test_snapshot_async() -> int:
    return 5


def test_assert_snapshot_multiple():
    snapshot_name_prefix = "test_snapshots_test_assert_snapshot_multiple"
    assert_json_snapshot("expected_result_1", snapshot_name=f"{snapshot_name_prefix}_1")
    assert_json_snapshot("expected_result_2", snapshot_name=f"{snapshot_name_prefix}_2")


def test_assert_json_snapshot_with_redactions():
    assert_json_snapshot(
        {
            "level_one": "left_alone",
            "also_level_one": "should_be_redacted",
        },
        redactions={".also_level_one": "[redacted]"},
    )


@snapshot(redactions={".also_level_one": "[redacted]"})
def test_snapshot_with_redactions():
    return {
        "level_one": "left_alone",
        "also_level_one": "should_be_redacted",
    }


@pytest.mark.skipif(POLARS_UNAVAILABLE, reason="Polars is an optional dependency")
@snapshot(redactions={"[1:][1]": "[redacted]"})
def test_assert_polars_dataframe_snapshot_redactions() -> pl.DataFrame:
    return pl.DataFrame(
        {
            "foo": [1, 2, 3, 4, 5],
            "bar": [6, 7, 8, 9, 10],
            "ham": ["a", "b", "c", "d", "e"],
        }
    )
