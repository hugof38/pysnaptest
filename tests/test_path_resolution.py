"""Snapshot path resolution must be independent of the process working directory.

``PYTEST_CURRENT_TEST``'s path component is relative to pytest's *rootdir*, not
to the process CWD. The old Rust layer probed it against ``current_dir()``, so
snapshots only resolved when pytest ran from its rootdir (or, by luck, the test
file's own parent); any other CWD raised ``FileNotFoundError`` and recorded zero
references, silently breaking ``pysnaptest unused`` (which runs with ``cwd=root``).

These tests run pytest from several CWDs and assert snapshots resolve, record
references, and get created next to the test file regardless.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path
from typing import Optional

import pytest

from pysnaptest._pysnaptest import SNAPSHOT_SUFFIX

REPO_ROOT = Path(__file__).resolve().parents[1]
EXAMPLE_ROOT = REPO_ROOT / "examples" / "my_project"
EXAMPLE_TEST = EXAMPLE_ROOT / "tests" / "test_main.py"
EXAMPLE_SNAPSHOTS = EXAMPLE_ROOT / "tests" / "snapshots"

# test_main -> 1 json snapshot; test_use_http_request -> json + mock request/response.
EXPECTED_REFERENCES = 4
CWDS = ["rootdir", "test_parent", "intermediate", "unrelated"]

pytestmark = pytest.mark.skipif(
    not EXAMPLE_TEST.exists(), reason="bundled example project is not available"
)


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
    collect the file from any ``cwd``. ``-o env=`` blanks the example's optional
    pytest-env config so the suite runs without that plugin.
    """

    env = {
        k: v
        for k, v in os.environ.items()
        if k not in ("INSTA_UPDATE", "PYSNAPTEST_TEST_FILE")
    }
    env["PYTHONPATH"] = os.pathsep.join(
        filter(None, [str(root), env.get("PYTHONPATH", "")])
    )
    env.update(extra_env or {})
    return subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            "-p",
            "no:cacheprovider",
            "-o",
            "env=",
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


def _example_cwd(kind: str, tmp_path: Path) -> Path:
    return {
        "rootdir": EXAMPLE_ROOT,  # always worked
        "test_parent": EXAMPLE_TEST.parent,  # worked only by accident (./<file> fallback)
        "intermediate": REPO_ROOT,  # between rootdir and test file: used to fail
        "unrelated": tmp_path,  # worst case
    }[kind]


@pytest.mark.parametrize("cwd_kind", CWDS)
def test_snapshots_resolve_regardless_of_cwd(cwd_kind: str, tmp_path: Path) -> None:
    ref_file = tmp_path / "referenced.txt"
    result = _run_pytest(
        _example_cwd(cwd_kind, tmp_path),
        EXAMPLE_ROOT,
        EXAMPLE_TEST,
        extra_env={"INSTA_SNAPSHOT_REFERENCES_FILE": str(ref_file)},
    )
    assert result.returncode == 0, result.stdout + result.stderr

    referenced = _referenced(ref_file)
    assert len(referenced) == EXPECTED_REFERENCES, referenced
    # Every reference lands in the example's real snapshots dir, proving the
    # folder came from the test file and not from the cwd.
    assert {r.parent for r in referenced} == {EXAMPLE_SNAPSHOTS.resolve()}


def test_reference_set_is_identical_across_cwds(tmp_path: Path) -> None:
    sets = {}
    for kind in CWDS:
        ref_file = tmp_path / f"{kind}.txt"
        result = _run_pytest(
            _example_cwd(kind, tmp_path),
            EXAMPLE_ROOT,
            EXAMPLE_TEST,
            extra_env={"INSTA_SNAPSHOT_REFERENCES_FILE": str(ref_file)},
        )
        assert result.returncode == 0, result.stdout + result.stderr
        sets[kind] = frozenset(_referenced(ref_file))

    assert len(set(sets.values())) == 1, sets


def _scaffold_project(root: Path) -> Path:
    """Create ``root/pytest.ini`` + ``root/pkg/nested/test_created_here.py``.

    The nesting reproduces the bug: ``root/pkg`` is neither the rootdir nor the
    test file's own parent.
    """

    (root / "pytest.ini").write_text("[pytest]\n", encoding="utf-8")
    test_dir = root / "pkg" / "nested"
    test_dir.mkdir(parents=True)
    test_file = test_dir / "test_created_here.py"
    test_file.write_text(
        "from pysnaptest import assert_json_snapshot\n\n"
        "def test_creates_snapshot():\n"
        "    assert_json_snapshot({'hello': 'world', 'n': 1})\n",
        encoding="utf-8",
    )
    return test_file


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

    cwd = {
        "rootdir": project,
        "intermediate": project / "pkg",
        "unrelated": tmp_path / "elsewhere",
    }[cwd_kind]
    cwd.mkdir(parents=True, exist_ok=True)

    result = _run_pytest(cwd, project, test_file, "--snapshot-update")
    assert result.returncode == 0, result.stdout + result.stderr

    created = list(snap_dir.glob(f"*{SNAPSHOT_SUFFIX}"))
    assert created, f"no snapshot created in {snap_dir}\n{result.stdout}"
    assert all(s.parent == snap_dir for s in created)
    assert any("test_created_here_test_creates_snapshot" in s.name for s in created)
    # Never created relative to the (differing) cwd.
    assert not (cwd / "snapshots").exists()
