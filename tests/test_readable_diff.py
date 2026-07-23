"""Tests for readable (CSV/JSON) diffs on binary DataFrame snapshots.

Equality is still insta's exact byte comparison of the stored binary (parquet
for pandas, ``bin`` for polars). When ``readable_diff`` is set and the bytes
differ, the committed snapshot is decompressed and a CSV/JSON unified diff is
attached to the ``AssertionError``.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from pysnaptest import assert_dataframe_snapshot
from pysnaptest._pysnaptest import render_text_diff

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


def _serialize(df, fmt: str) -> bytes:
    """Serialize a DataFrame the same way the assertion path does."""

    if fmt == "parquet":
        return df.to_parquet(engine="pyarrow")
    return df.serialize(format="binary")  # polars "bin"


def _commit_snapshot(df, snap_dir: Path, name: str, fmt: str) -> None:
    """Hand-write the committed binary snapshot (insta metadata + sidecar).

    This mirrors exactly what insta writes for a binary snapshot, so the
    assertion path compares against it byte-for-byte without needing insta's
    update mode (whose behavior insta caches per-process).
    """

    snap_dir.mkdir(parents=True, exist_ok=True)
    metadata = snap_dir / f"pysnaptest__{name}@pysnap.snap"
    metadata.write_text(
        f"---\nsource: src/lib.rs\nextension: {fmt}\nsnapshot_kind: binary\n---\n",
        encoding="utf-8",
    )
    sidecar = snap_dir / f"pysnaptest__{name}@pysnap.snap.{fmt}"
    sidecar.write_bytes(_serialize(df, fmt))


def test_render_text_diff_basic():
    diff = render_text_diff("a\nb\nc\n", "a\nB\nc\n", "committed", "new")
    assert "committed" in diff
    assert "new" in diff
    assert "-b" in diff
    assert "+B" in diff


def test_render_text_diff_default_labels():
    diff = render_text_diff("x\n", "y\n")
    assert "committed" in diff
    assert "new" in diff


@pytest.mark.skipif(PANDAS_UNAVAILABLE, reason="pandas is an optional dependency")
def test_pandas_parquet_readable_diff_csv(tmp_path: Path):
    snap_dir = tmp_path / "snapshots"
    name = "test_readable_diff_pandas_csv"
    old = pd.DataFrame({"id": [1, 2], "name": ["foo", "bar"]})
    _commit_snapshot(old, snap_dir, name, "parquet")

    new = pd.DataFrame({"id": [1, 2], "name": ["foo", "CHANGED"]})
    with pytest.raises(AssertionError) as exc:
        assert_dataframe_snapshot(
            new,
            snapshot_path=str(snap_dir),
            snapshot_name=name,
            dataframe_snapshot_format="parquet",
            readable_diff="csv",
        )

    message = str(exc.value)
    assert "readable diff below" in message
    assert "bar" in message  # value from the committed snapshot
    assert "CHANGED" in message  # value from the new DataFrame


@pytest.mark.skipif(PANDAS_UNAVAILABLE, reason="pandas is an optional dependency")
def test_pandas_parquet_readable_diff_json(tmp_path: Path):
    snap_dir = tmp_path / "snapshots"
    name = "test_readable_diff_pandas_json"
    old = pd.DataFrame({"id": [1], "name": ["foo"]})
    _commit_snapshot(old, snap_dir, name, "parquet")

    new = pd.DataFrame({"id": [1], "name": ["bar"]})
    with pytest.raises(AssertionError) as exc:
        assert_dataframe_snapshot(
            new,
            snapshot_path=str(snap_dir),
            snapshot_name=name,
            dataframe_snapshot_format="parquet",
            readable_diff="json",
        )

    message = str(exc.value)
    assert '"name"' in message  # JSON-rendered rows
    assert "foo" in message
    assert "bar" in message


@pytest.mark.skipif(PANDAS_UNAVAILABLE, reason="pandas is an optional dependency")
def test_pandas_parquet_readable_diff_matches(tmp_path: Path):
    snap_dir = tmp_path / "snapshots"
    name = "test_readable_diff_pandas_match"
    df = pd.DataFrame({"id": [1, 2], "name": ["foo", "bar"]})
    _commit_snapshot(df, snap_dir, name, "parquet")

    # Same DataFrame -> identical bytes -> passes, no diff raised.
    assert_dataframe_snapshot(
        df,
        snapshot_path=str(snap_dir),
        snapshot_name=name,
        dataframe_snapshot_format="parquet",
        readable_diff="csv",
    )


@pytest.mark.skipif(PANDAS_UNAVAILABLE, reason="pandas is an optional dependency")
def test_pandas_parquet_byte_only_default(tmp_path: Path):
    snap_dir = tmp_path / "snapshots"
    name = "test_readable_diff_pandas_byteonly"
    old = pd.DataFrame({"id": [1], "name": ["foo"]})
    _commit_snapshot(old, snap_dir, name, "parquet")

    new = pd.DataFrame({"id": [1], "name": ["bar"]})
    with pytest.raises(AssertionError) as exc:
        assert_dataframe_snapshot(
            new,
            snapshot_path=str(snap_dir),
            snapshot_name=name,
            dataframe_snapshot_format="parquet",
        )

    # Without readable_diff the message stays byte-only (no rendered diff).
    assert "readable diff below" not in str(exc.value)


@pytest.mark.skipif(POLARS_UNAVAILABLE, reason="polars is an optional dependency")
def test_polars_bin_readable_diff_csv(tmp_path: Path):
    snap_dir = tmp_path / "snapshots"
    name = "test_readable_diff_polars_csv"
    old = pl.DataFrame({"id": [1, 2], "name": ["foo", "bar"]})
    _commit_snapshot(old, snap_dir, name, "bin")

    new = pl.DataFrame({"id": [1, 2], "name": ["foo", "CHANGED"]})
    with pytest.raises(AssertionError) as exc:
        assert_dataframe_snapshot(
            new,
            snapshot_path=str(snap_dir),
            snapshot_name=name,
            dataframe_snapshot_format="bin",
            readable_diff="csv",
        )

    message = str(exc.value)
    assert "readable diff below" in message
    assert "bar" in message
    assert "CHANGED" in message
