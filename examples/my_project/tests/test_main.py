from pysnaptest import snapshot, patch_json_snapshot
from my_project.main import main, use_http_request


@snapshot()
def test_main():
    return main()


@patch_json_snapshot("my_project.main.http_request")
@snapshot()
def test_use_http_request():
    return use_http_request()
