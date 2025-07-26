import json
import os
import random
import string
import time
import tracemalloc

import pytest

from pysnaptest import assert_json_snapshot
from syrupy.assertion import SnapshotAssertion as SyrupSnapshot


def generate_json(n: int) -> dict:
    return {str(i): [i] * i for i in range(n)}


@pytest.mark.benchmark
@pytest.mark.parametrize("size", [10, 1000])
def test_pysnaptest_benchmark(tmp_path, benchmark, size):
    data = generate_json(size)
    benchmark(lambda: assert_json_snapshot(data, allow_duplicates=True))


@pytest.mark.benchmark
@pytest.mark.parametrize("size", [10, 1000])
def test_syrupy_benchmark(snapshot: SyrupSnapshot, benchmark, size):
    data = generate_json(size)

    def do_assert(data):
        assert data == snapshot(name=f"benchmark_assert-{size}")

    benchmark(lambda: do_assert(data))
