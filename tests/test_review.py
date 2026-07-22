"""Tests for the cargo-free review workflow (pytest plugin, CLI, and helpers)."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

from pysnaptest import (
    accept_pending_snapshot,
    find_pending_snapshots,
    print_pending_diff,
    reject_pending_snapshot,
)


def _write_project(tmp_path: Path, value: str) -> None:
    (tmp_path / "test_demo.py").write_text(
        "from pysnaptest import assert_snapshot\n"
        "def test_demo():\n"
        f"    assert_snapshot({value!r})\n"
    )


def _run_pytest(tmp_path: Path, *args: str) -> subprocess.CompletedProcess[str]:
    # Disable bytecode caching so rewriting test_demo.py within the same
    # filesystem timestamp never reuses a stale .pyc (a test-harness artifact,
    # not something real edits hit).
    env = {
        **os.environ,
        "INSTA_WORKSPACE_ROOT": str(tmp_path),
        "PYTHONDONTWRITEBYTECODE": "1",
    }
    return subprocess.run(
        [sys.executable, "-m", "pytest", "test_demo.py", "-q", *args],
        cwd=tmp_path,
        env=env,
        capture_output=True,
        text=True,
    )


def _make_pending(tmp_path: Path, value: str = "VERSION_A") -> Path:
    """Create a genuine pending snapshot via a subprocess run and return its path."""

    _write_project(tmp_path, value)
    result = _run_pytest(tmp_path, "--snapshot-new")
    assert result.returncode != 0, result.stdout + result.stderr
    pending = find_pending_snapshots(str(tmp_path))
    assert len(pending) == 1, pending
    return pending[0]


def test_find_pending_snapshots_ignores_foreign(tmp_path: Path) -> None:
    snapshots = tmp_path / "snapshots"
    snapshots.mkdir()
    (snapshots / "mod__test@pysnap.snap.new").write_text("---\n---\nx\n")
    (snapshots / "foreign.snap.new").write_text("not ours")
    (snapshots / "mod__test@pysnap.snap").write_text("committed")

    found = find_pending_snapshots(str(tmp_path))

    assert [p.name for p in found] == ["mod__test@pysnap.snap.new"]


def test_accept_persists_and_cleans_up(tmp_path: Path) -> None:
    pending = _make_pending(tmp_path, "VERSION_A")

    target = accept_pending_snapshot(pending)

    assert target.exists()
    assert not pending.exists()
    # The committed snapshot now satisfies the assertion.
    rerun = _run_pytest(tmp_path)
    assert rerun.returncode == 0, rerun.stdout + rerun.stderr


def test_reject_deletes_pending(tmp_path: Path) -> None:
    pending = _make_pending(tmp_path, "VERSION_A")
    target = pending.with_suffix("")

    reject_pending_snapshot(pending)

    assert not pending.exists()
    assert not target.exists()


def test_print_pending_diff_renders_insta_diff(tmp_path: Path, capfd) -> None:
    # Commit a baseline, then create a changed pending snapshot.
    accept_pending_snapshot(_make_pending(tmp_path, "VERSION_A"))
    _write_project(tmp_path, "VERSION_B")
    _run_pytest(tmp_path, "--snapshot-new")
    pending = find_pending_snapshots(str(tmp_path))[0]

    print_pending_diff(pending, str(tmp_path))

    out = capfd.readouterr().out
    # insta's own diff shows the removed old line and the added new line.
    assert "-VERSION_A" in out or "VERSION_A" in out
    assert "VERSION_B" in out


def test_cli_review_accept(tmp_path: Path) -> None:
    accept_pending_snapshot(_make_pending(tmp_path, "VERSION_A"))
    _write_project(tmp_path, "VERSION_B")
    _run_pytest(tmp_path, "--snapshot-new")

    result = subprocess.run(
        [sys.executable, "-m", "pysnaptest", "--root", str(tmp_path), "review"],
        input="a\n",
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stdout + result.stderr
    assert find_pending_snapshots(str(tmp_path)) == []
    committed = next((tmp_path / "snapshots").glob("*@pysnap.snap"))
    assert "VERSION_B" in committed.read_text()


def test_snapshot_update_flag_updates_in_place(tmp_path: Path) -> None:
    # Establish a committed baseline.
    pending = _make_pending(tmp_path, "VERSION_A")
    accept_pending_snapshot(pending)

    # Change the value and update in place with --snapshot-update, which passes.
    _write_project(tmp_path, "VERSION_B")
    result = _run_pytest(tmp_path, "--snapshot-update")

    assert result.returncode == 0, result.stdout + result.stderr
    assert find_pending_snapshots(str(tmp_path)) == []
    committed = next((tmp_path / "snapshots").glob("*@pysnap.snap"))
    assert "VERSION_B" in committed.read_text()
