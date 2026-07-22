"""Pytest plugin for pysnaptest.

Registered via the ``pytest11`` entry point, this plugin adds cargo-free
snapshot-management flags to ``pytest``:

* ``--snapshot-update`` — update snapshots in place and pass (sets
  ``INSTA_UPDATE=always`` before any assertion runs).
* ``--snapshot-new`` — record changed/new snapshots as pending ``*.snap.new``
  files instead (sets ``INSTA_UPDATE=new``); accept them with
  ``pysnaptest accept``.

insta does the actual work (diffing, writing, format); this plugin only selects
the update mode. The environment variable is set in :func:`pytest_configure`,
which runs before the first assertion — insta caches its update configuration
per workspace on first read, so setting it any later would have no effect.
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import pytest


def pytest_addoption(parser: "pytest.Parser") -> None:
    """Register pysnaptest's command-line options."""

    group = parser.getgroup("pysnaptest", "snapshot testing (pysnaptest)")
    group.addoption(
        "--snapshot-update",
        action="store_true",
        default=False,
        help="Update snapshots in place and pass (INSTA_UPDATE=always).",
    )
    group.addoption(
        "--snapshot-new",
        action="store_true",
        default=False,
        help="Record changed snapshots as pending *.snap.new files (INSTA_UPDATE=new).",
    )


def pytest_configure(config: "pytest.Config") -> None:
    """Set the insta update mode before any assertion runs.

    ``--snapshot-update`` takes precedence over ``--snapshot-new`` if both are
    given. An ``INSTA_UPDATE`` value already present in the environment is left
    untouched so explicit user configuration wins.
    """

    if os.environ.get("INSTA_UPDATE"):
        return
    if config.getoption("--snapshot-update"):
        os.environ["INSTA_UPDATE"] = "always"
    elif config.getoption("--snapshot-new"):
        os.environ["INSTA_UPDATE"] = "new"
