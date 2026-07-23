"""Tests for obsolete-snapshot detection (``pysnaptest.unused``)."""

from __future__ import annotations

import subprocess
import sys
import textwrap
from pathlib import Path

from pysnaptest.unused import (
    delete_snapshot,
    discover_snapshot_dirs,
    find_unused_snapshots,
    owning_stem,
    read_referenced,
    sibling_test_stems,
    snapshot_files,
)


def _write_snapshot(path: Path, body: str = "value") -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"---\nsource: t\n---\n{body}\n", encoding="utf-8")
    return path


def _write_binary_snapshot(path: Path, extension: str, data: bytes) -> Path:
    """Write a binary snapshot (insta metadata + sidecar) like insta does."""

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        f"---\nsource: t\nextension: {extension}\nsnapshot_kind: binary\n---\n",
        encoding="utf-8",
    )
    (path.parent / f"{path.name}.{extension}").write_bytes(data)
    return path


def test_owning_stem_prefers_longest_match():
    snap = Path("pysnaptest__test_snapshots_test_thing@pysnap.snap")
    # Both "test" and "test_snapshots" are plausible prefixes; the longer wins.
    assert owning_stem(snap, {"test", "test_snapshots"}) == "test_snapshots"


def test_owning_stem_exact_match():
    snap = Path("mod__test_main@pysnap.snap")
    assert owning_stem(snap, {"test_main"}) == "test_main"


def test_owning_stem_no_known_stem():
    snap = Path("mod__test_orphan_thing@pysnap.snap")
    assert owning_stem(snap, {"test_other"}) is None


def test_owning_stem_ignores_non_snapshot_name():
    assert owning_stem(Path("not-a-snapshot.txt"), {"test_main"}) is None


def test_sibling_test_stems(tmp_path: Path):
    (tmp_path / "test_a.py").write_text("", encoding="utf-8")
    (tmp_path / "test_b.py").write_text("", encoding="utf-8")
    (tmp_path / "notes.md").write_text("", encoding="utf-8")
    snapshots = tmp_path / "snapshots"
    snapshots.mkdir()
    assert sibling_test_stems(snapshots) == {"test_a", "test_b"}


def test_snapshot_files_matches_only_committed(tmp_path: Path):
    snaps = tmp_path / "snapshots"
    committed = _write_snapshot(snaps / "mod__test_a_x@pysnap.snap")
    _write_snapshot(snaps / "mod__test_a_x@pysnap.snap.new")  # pending, ignored
    found = snapshot_files([snaps])
    assert found == {committed.resolve()}


def test_read_referenced_missing_file(tmp_path: Path):
    assert read_referenced(tmp_path / "does-not-exist.txt") == set()


def test_read_referenced_parses_lines(tmp_path: Path):
    a = _write_snapshot(tmp_path / "snapshots" / "mod__test_a_x@pysnap.snap")
    ref = tmp_path / "refs.txt"
    ref.write_text(f"{a}\n\n{a}\n", encoding="utf-8")
    assert read_referenced(ref) == {a.resolve()}


def test_find_unused_only_flags_ran_stems(tmp_path: Path):
    (tmp_path / "test_a.py").write_text("", encoding="utf-8")
    (tmp_path / "test_b.py").write_text("", encoding="utf-8")
    snaps = tmp_path / "snapshots"
    used_a = _write_snapshot(snaps / "mod__test_a_used@pysnap.snap")
    unused_a = _write_snapshot(snaps / "mod__test_a_orphan@pysnap.snap")
    # Owned by test_b, which did NOT run: must not be flagged.
    unused_b = _write_snapshot(snaps / "mod__test_b_orphan@pysnap.snap")

    result = find_unused_snapshots(
        referenced=[used_a],
        snapshot_dirs=[snaps],
        ran_stems=["test_a"],
    )
    assert result == [unused_a.resolve()]
    assert unused_b.resolve() not in result


def test_delete_snapshot_removes_sidecars(tmp_path: Path):
    snaps = tmp_path / "snapshots"
    meta = _write_binary_snapshot(
        snaps / "mod__test_a_x@pysnap.snap", "parquet", b"binary"
    )
    sidecar = snaps / "mod__test_a_x@pysnap.snap.parquet"

    removed = delete_snapshot(meta)

    assert set(removed) == {meta, sidecar}
    assert not meta.exists()
    assert not sidecar.exists()


def test_delete_snapshot_leaves_unrelated_siblings(tmp_path: Path):
    # Deletion resolves the sidecar through insta's build_binary_path, so a
    # sibling pending file that merely shares the name prefix is never touched.
    snaps = tmp_path / "snapshots"
    meta = _write_snapshot(snaps / "mod__test_a_x@pysnap.snap")
    pending = _write_snapshot(snaps / "mod__test_a_x@pysnap.snap.new")

    removed = delete_snapshot(meta)

    assert removed == [meta]
    assert not meta.exists()
    assert pending.exists()


def test_discover_snapshot_dirs(tmp_path: Path):
    a = tmp_path / "pkg" / "snapshots"
    b = tmp_path / "other" / "snapshots"
    a.mkdir(parents=True)
    b.mkdir(parents=True)
    (tmp_path / "not-snapshots").mkdir()
    assert discover_snapshot_dirs(tmp_path) == sorted([a, b])


# --- End-to-end CLI test ----------------------------------------------------


def _make_project(root: Path) -> None:
    """Create a minimal project with one passing snapshot test."""

    (root / "pytest.ini").write_text(
        "[pytest]\nINSTA_WORKSPACE_ROOT = .\n", encoding="utf-8"
    )
    (root / "test_thing.py").write_text(
        textwrap.dedent(
            """
            from pysnaptest import assert_snapshot

            def test_thing():
                assert_snapshot("kept")
            """
        ),
        encoding="utf-8",
    )


def _run_cli(root: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-m", "pysnaptest", "unused", *args],
        cwd=str(root),
        capture_output=True,
        text=True,
    )


def test_cli_unused_reports_and_deletes(tmp_path: Path):
    _make_project(tmp_path)

    # Record the initial snapshot for the real test.
    subprocess.run(
        [sys.executable, "-m", "pytest", "--snapshot-update", "-q"],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
    )
    snaps = tmp_path / "snapshots"
    kept = snaps / "pysnaptest__test_thing_test_thing@pysnap.snap"
    assert kept.exists()

    # Add an orphan snapshot owned by the same (existing) test file.
    orphan = _write_snapshot(snaps / "pysnaptest__test_thing_test_gone@pysnap.snap")

    report = _run_cli(tmp_path)
    assert report.returncode == 1
    assert "Found 1 unused snapshot" in report.stdout
    assert orphan.exists()

    deleted = _run_cli(tmp_path, "--delete")
    assert deleted.returncode == 0
    assert "Deleting 1 unused snapshot" in deleted.stdout
    assert not orphan.exists()
    assert kept.exists()  # referenced snapshot preserved
