from unittest.mock import patch
from pysnaptest import snapshot_mock, assert_snapshot
from mock.to_mock import function_to_mock, main


def test_mock_snapshot():
    with patch(
        "mock.to_mock.function_to_mock", side_effect=snapshot_mock(function_to_mock)
    ):
        assert_snapshot(main())
