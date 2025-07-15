from pysnaptest import snapshot, mock_json_snapshot
from my_project.main import main, use_http_request, http_request
from unittest.mock import patch

@snapshot()
def test_main():
    return main()

@snapshot()
def test_use_http_request():
    with patch("my_project.main.http_request", side_effect=mock_json_snapshot(http_request)):
        return use_http_request()

