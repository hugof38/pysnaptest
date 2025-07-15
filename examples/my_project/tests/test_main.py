from pysnaptest import snapshot, snapshot_json_patch
from my_project.main import main, use_http_request


@snapshot()
def test_main():
    return main()


@snapshot_json_patch("my_project.main.http_request")
@snapshot()
def test_use_http_request():
    return use_http_request()
