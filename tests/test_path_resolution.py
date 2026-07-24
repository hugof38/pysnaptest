"""Snapshot path resolution must be independent of the process working directory.

``PYTEST_CURRENT_TEST``'s path component is relative to pytest's *rootdir*, not
to the process CWD. The old Rust layer probed it against ``current_dir()``, so
snapshots only resolved when pytest ran from its rootdir (or, by luck, the test
file's own parent); any other CWD raised ``FileNotFoundError`` and recorded zero
references, silently breaking ``pysnaptest unused`` (which runs with ``cwd=root``).

These tests scaffold a self-contained project (no third-party deps) and run
pytest from several CWDs, asserting snapshots resolve, record references, and get
created next to the test file regardless of where pytest was launched.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path
from typing import Optional, Tuple

import pytest

from pysnaptest._pysnaptest import SNAPSHOT_SUFFIX

# The scaffolded test module makes two json-snapshot assertions -> two references.
EXPECTED_REFERENCES = 2
CWDS = ["rootdir", "test_parent", "intermediate", "unrelated"]


def _run_pytest(
    cwd: Path,
    root: Path,
    test_file: Path,
    *extra: str,
    extra_env: Optional[dict] = None,
) -> subprocess.CompletedProcess:
    """Run pytest against ``test_file`` with rootdir pinned to ``root``.

    Pinning ``--rootdir`` keeps ``PYTEST_CURRENT_TEST`` rootdir-relative (the
    condition that used to break CWD probing); the absolute test path lets pytest
    collect the file from any ``cwd``.
    """

    env = {
        k: v
        for k, v in os.environ.items()
        if k not in ("INSTA_UPDATE", "PYSNAPTEST_TEST_FILE")
    }
    env.update(extra_env or {})
    return subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            "-p",
            "no:cacheprovider",
            "--rootdir",
            str(root),
            str(test_file),
            "-q",
            *extra,
        ],
        cwd=str(cwd),
        env=env,
        capture_output=True,
        text=True,
    )


def _referenced(ref_file: Path) -> set:
    if not ref_file.exists():
        return set()
    return {
        Path(line).resolve()
        for line in ref_file.read_text(encoding="utf-8").splitlines()
        if line.strip()
    }


def _scaffold_project(root: Path) -> Path:
    """Create ``root/pytest.ini`` + ``root/pkg/nested/test_created_here.py``.

    The nesting is what reproduces the bug: ``root/pkg`` is neither the rootdir
    nor the test file's own parent.
    """

    (root / "pytest.ini").write_text("[pytest]\n", encoding="utf-8")
    test_dir = root / "pkg" / "nested"
    test_dir.mkdir(parents=True)
    test_file = test_dir / "test_created_here.py"
    test_file.write_text(
        "from pysnaptest import assert_json_snapshot\n\n"
        "def test_alpha():\n"
        "    assert_json_snapshot({'hello': 'world', 'n': 1})\n\n"
        "def test_beta():\n"
        "    assert_json_snapshot(['a', 'b', 'c'])\n",
        encoding="utf-8",
    )
    return test_file


def _cwd(kind: str, project: Path, test_file: Path, tmp_path: Path) -> Path:
    cwd = {
        "rootdir": project,  # always worked
        "test_parent": test_file.parent,  # worked only by accident (./<file> fallback)
        "intermediate": project / "pkg",  # between rootdir and test file: used to fail
        "unrelated": tmp_path / "elsewhere",  # worst case
    }[kind]
    cwd.mkdir(parents=True, exist_ok=True)
    return cwd


@pytest.fixture
def committed_project(tmp_path: Path) -> Tuple[Path, Path, Path]:
    """Scaffold a project and create its committed snapshots from the rootdir."""

    project = tmp_path / "proj"
    project.mkdir()
    test_file = _scaffold_project(project)
    result = _run_pytest(project, project, test_file, "--snapshot-update")
    assert result.returncode == 0, result.stdout + result.stderr
    return project, test_file, test_file.parent / "snapshots"


@pytest.mark.parametrize("cwd_kind", CWDS)
def test_snapshots_resolve_regardless_of_cwd(
    cwd_kind: str, committed_project: Tuple[Path, Path, Path], tmp_path: Path
) -> None:
    project, test_file, snap_dir = committed_project
    ref_file = tmp_path / "referenced.txt"

    result = _run_pytest(
        _cwd(cwd_kind, project, test_file, tmp_path),
        project,
        test_file,
        extra_env={"INSTA_SNAPSHOT_REFERENCES_FILE": str(ref_file)},
    )
    assert result.returncode == 0, result.stdout + result.stderr

    referenced = _referenced(ref_file)
    assert len(referenced) == EXPECTED_REFERENCES, referenced
    # Every reference lands in the test file's own snapshots dir, proving the
    # folder came from the test file and not from the cwd.
    assert {r.parent for r in referenced} == {snap_dir.resolve()}


def test_reference_set_is_identical_across_cwds(
    committed_project: Tuple[Path, Path, Path], tmp_path: Path
) -> None:
    project, test_file, _ = committed_project
    sets = {}
    for kind in CWDS:
        ref_file = tmp_path / f"{kind}.txt"
        result = _run_pytest(
            _cwd(kind, project, test_file, tmp_path),
            project,
            test_file,
            extra_env={"INSTA_SNAPSHOT_REFERENCES_FILE": str(ref_file)},
        )
        assert result.returncode == 0, result.stdout + result.stderr
        sets[kind] = frozenset(_referenced(ref_file))

    assert len(set(sets.values())) == 1, sets


@pytest.mark.parametrize("cwd_kind", ["rootdir", "intermediate", "unrelated"])
def test_new_snapshot_is_created_next_to_test_file(
    cwd_kind: str, tmp_path: Path
) -> None:
    """A new snapshot must be written into ``<test_dir>/snapshots`` like insta,
    whatever directory pytest was launched from."""

    project = tmp_path / "proj"
    project.mkdir()
    test_file = _scaffold_project(project)
    snap_dir = test_file.parent / "snapshots"
    cwd = _cwd(cwd_kind, project, test_file, tmp_path)

    result = _run_pytest(cwd, project, test_file, "--snapshot-update")
    assert result.returncode == 0, result.stdout + result.stderr

    created = list(snap_dir.glob(f"*{SNAPSHOT_SUFFIX}"))
    assert created, f"no snapshot created in {snap_dir}\n{result.stdout}"
    assert all(s.parent == snap_dir for s in created)
    assert any("test_created_here_test_alpha" in s.name for s in created)
    # Never created relative to the (differing) cwd.
    assert not (cwd / "snapshots").exists()
