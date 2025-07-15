import requests
from typing import Any


def http_request(url: str) -> dict[str, Any]:
    print(f"Making request to url {url}")
    resp = requests.get(url)
    assert resp.status_code == 200
    return resp.json()


def use_http_request() -> int:
    resp = http_request("https://jsonplaceholder.typicode.com/todos/1")
    return resp["id"] + 1


def main():
    return {"main": "result"}
