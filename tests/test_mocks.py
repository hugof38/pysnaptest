"""Tests for the Python-orchestrated mock layer.

These exercise `mock_json_snapshot` beyond the plain-dict cases already
covered in `test_snapshots.py`: Pydantic model arguments/results (the reason
values are normalized through `to_jsonable` before crossing into Rust) and
`async def` functions.
"""

from __future__ import annotations

import pytest

from pysnaptest import mock_json_snapshot

pydantic = pytest.importorskip("pydantic")


class User(pydantic.BaseModel):
    id: int
    name: str


def test_mock_json_snapshot_pydantic_args_and_result():
    def fetch_user(user: User) -> User:
        return User(id=user.id, name=user.name.upper())

    mocked = mock_json_snapshot(func=fetch_user)
    result = mocked(User(id=1, name="ada"))

    assert result == {"id": 1, "name": "ADA"}


@pytest.mark.asyncio
async def test_mock_json_snapshot_async():
    async def fetch_user(user_id: int) -> User:
        return User(id=user_id, name="grace")

    mocked = mock_json_snapshot(func=fetch_user)
    result = await mocked(7)

    assert result == {"id": 7, "name": "grace"}
