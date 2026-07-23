"""Detect snapshot files that no test referenced ("obsolete" snapshots).

This leans on insta's own machinery. When the ``INSTA_SNAPSHOT_REFERENCES_FILE``
environment variable is set, insta appends the path of every snapshot file it
touches during an assertion to that file -- this is exactly the mechanism
``cargo insta test --unreferenced`` uses. The ``pysnaptest unused`` CLI command
points that variable at a temp file, runs your test suite once, and then treats
anything on disk that isn't listed as a candidate for deletion.

Mock *replay* is covered too: :func:`pysnaptest.mocks.mock_json_snapshot` reads
a recorded response snapshot without going through an insta assertion, so the
Rust ``read_json_snapshot`` primitive appends that path to the same reference
file (just as insta's own ``memoize_snapshot_file`` does), keeping the
referenced set complete.

Because the CLI runs the whole suite, every test file runs, so scoping is simple:
a snapshot is reported only when its owning test file still exists next to the
snapshot directory (:func:`find_unused_snapshots` takes the set of stems that
ran, which the CLI fills with every discovered test module).
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import re
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Sequence, Set

from ._pysnaptest import delete_snapshot as _delete_snapshot
from .review import SNAPSHOT_GLOB, SNAPSHOT_SUFFIX, _root

#: Splits a snapshot filename into its Rust module prefix and the remainder,
#: which starts with the owning test file's stem: ``<stem>_<test_name>...``.
_NAME_RE = re.compile(
    rf"^(?P<module>.+?)__(?P<remainder>.+){re.escape(SNAPSHOT_SUFFIX)}$"
)


def read_referenced(reference_file: str | Path) -> Set[Path]:
    """Read the set of referenced snapshot paths insta recorded.

    Args:
        reference_file: Path insta appended referenced snapshots to (one per
            line). A missing file yields an empty set.

    Returns:
        Set[Path]: Resolved paths of every referenced snapshot.
    """

    referenced: Set[Path] = set()
    try:
        with open(reference_file, encoding="utf-8") as handle:
            for line in handle:
                stripped = line.strip()
                if stripped:
                    referenced.add(Path(stripped).resolve())
    except FileNotFoundError:
        pass
    return referenced


def discover_snapshot_dirs(root: str | Path) -> List[Path]:
    """Find every ``snapshots`` directory under ``root``.

    Args:
        root: Directory to search recursively.

    Returns:
        List[Path]: Sorted ``snapshots`` directories.
    """

    return sorted(p for p in Path(root).rglob("snapshots") if p.is_dir())


def snapshot_files(directories: Iterable[Path]) -> Set[Path]:
    """Return every committed pysnaptest snapshot under ``directories``.

    Args:
        directories: Snapshot directories to scan (typically ``<testdir>/snapshots``).

    Returns:
        Set[Path]: Resolved paths to ``*@pysnap.snap`` metadata files.
    """

    found: Set[Path] = set()
    for directory in directories:
        if not directory.is_dir():
            continue
        for path in directory.glob(SNAPSHOT_GLOB):
            if path.is_file():
                found.add(path.resolve())
    return found


def owning_stem(snapshot_path: Path, known_stems: Iterable[str]) -> Optional[str]:
    """Resolve which test-file stem owns ``snapshot_path``.

    A snapshot filename looks like ``<module>__<stem>_<test_name>...@pysnap.snap``.
    Because both ``<stem>`` and ``<test_name>`` may contain underscores, the stem
    is ambiguous in isolation; it is resolved by matching against the set of
    ``known_stems`` (the test modules that exist next to the snapshot directory)
    and preferring the longest stem that fits on an underscore boundary. That way
    ``test`` never steals ``test_snapshots``'s files.

    Args:
        snapshot_path: Path to a ``*@pysnap.snap`` file.
        known_stems: Candidate test-file stems (e.g. ``{"test_main", "test_api"}``).

    Returns:
        Optional[str]: The owning stem, or ``None`` if no known stem matches.
    """

    match = _NAME_RE.match(snapshot_path.name)
    if match is None:
        return None
    remainder = match.group("remainder")

    best: Optional[str] = None
    for stem in known_stems:
        if remainder == stem or remainder.startswith(f"{stem}_"):
            if best is None or len(stem) > len(best):
                best = stem
    return best


def sibling_test_stems(directory: Path) -> Set[str]:
    """Return the stems of test modules living next to a ``snapshots`` directory.

    Snapshots are stored in ``<testdir>/snapshots``; the test modules that own
    them are the ``.py`` files directly in ``<testdir>``. Knowing every stem that
    exists there (not just the ones that ran) is what lets :func:`owning_stem`
    disambiguate overlapping names.

    Args:
        directory: A ``snapshots`` directory.

    Returns:
        Set[str]: Stems of sibling ``.py`` files.
    """

    parent = directory.parent
    if not parent.is_dir():
        return set()
    return {p.stem for p in parent.glob("*.py")}


def find_unused_snapshots(
    referenced: Iterable[Path],
    snapshot_dirs: Iterable[Path],
    ran_stems: Iterable[str],
) -> List[Path]:
    """Return committed snapshots that ran-tests own but never referenced.

    A snapshot is reported only when its owning test file both exists next to the
    snapshot directory and is among ``ran_stems``. Snapshots owned by stems that
    did not run (or whose test file is gone) are left alone, so partial test runs
    do not produce false positives.

    Args:
        referenced: Snapshot metadata paths insta recorded as referenced.
        snapshot_dirs: The ``snapshots`` directories to scan.
        ran_stems: Test-file stems that actually ran this session.

    Returns:
        List[Path]: Sorted, resolved paths to unused snapshot metadata files.
    """

    referenced_resolved = {Path(p).resolve() for p in referenced}
    ran = set(ran_stems)
    dirs = list(snapshot_dirs)

    stems_by_dir: Dict[Path, Set[str]] = {d: sibling_test_stems(d) for d in dirs}

    unused: List[Path] = []
    for path in snapshot_files(dirs):
        if path in referenced_resolved:
            continue
        known = stems_by_dir.get(path.parent, set()) or sibling_test_stems(path.parent)
        stem = owning_stem(path, known)
        if stem is not None and stem in ran:
            unused.append(path)
    return sorted(unused)


def delete_snapshot(snapshot_path: Path) -> List[Path]:
    """Delete a snapshot metadata file and its binary sidecar (if any).

    Deletion is delegated to the Rust ``delete_snapshot`` primitive, which
    resolves any binary sidecar through insta's own ``build_binary_path`` (the
    same code path that cleans up pending files), so only the data file insta
    actually wrote is removed.

    Args:
        snapshot_path: Path to a ``*@pysnap.snap`` metadata file.

    Returns:
        List[Path]: The files that were removed.
    """

    return [Path(p) for p in _delete_snapshot(str(snapshot_path))]


def collect_references(
    reference_file: Path,
    pytest_args: Sequence[str],
    root: Path,
) -> int:
    """Run the test suite once, recording every referenced snapshot.

    Points insta's ``INSTA_SNAPSHOT_REFERENCES_FILE`` at ``reference_file`` and
    runs ``python -m pytest`` (plus any ``pytest_args``) as a subprocess, so
    insta appends every snapshot it touches. Mirrors what ``cargo insta test``
    does before checking for unreferenced snapshots.

    Args:
        reference_file: File insta should append referenced snapshots to.
        pytest_args: Extra arguments forwarded to pytest (e.g. a test path).
        root: Working directory to run pytest in.

    Returns:
        int: pytest's exit code.
    """

    env = dict(os.environ)
    env["INSTA_SNAPSHOT_REFERENCES_FILE"] = str(reference_file)
    completed = subprocess.run(
        [sys.executable, "-m", "pytest", *pytest_args],
        cwd=str(root),
        env=env,
    )
    return completed.returncode


def unused_snapshots(
    root: Optional[str] = None, pytest_args: Sequence[str] = ()
) -> List[Path]:
    """Return snapshots no test referenced after running the suite under ``root``.

    Runs the test suite once (via :func:`collect_references`) to learn which
    snapshots are referenced, then diffs that against the snapshots on disk.
    Because the whole suite runs, every test file counts as "ran", so a snapshot
    is reported whenever its owning test module still exists.

    Args:
        root: Project directory to run pytest in and scan for ``snapshots`` dirs.
            Defaults to ``$INSTA_WORKSPACE_ROOT`` or the current directory, matching
            the other ``pysnaptest`` subcommands.
        pytest_args: Extra arguments forwarded to pytest.

    Returns:
        List[Path]: Sorted paths to unreferenced snapshot metadata files.
    """

    root_path = _root(root)
    snapshot_dirs = discover_snapshot_dirs(root_path)
    ran_stems: Set[str] = set()
    for directory in snapshot_dirs:
        ran_stems |= sibling_test_stems(directory)

    handle = tempfile.NamedTemporaryFile(
        prefix="pysnaptest-refs-", suffix=".txt", delete=False
    )
    handle.close()
    reference_file = Path(handle.name)
    try:
        collect_references(reference_file, pytest_args, root_path)
        referenced = read_referenced(reference_file)
    finally:
        try:
            reference_file.unlink()
        except OSError:
            pass

    return find_unused_snapshots(referenced, snapshot_dirs, ran_stems)
