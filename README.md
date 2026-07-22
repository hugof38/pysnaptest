# PySnapTest

`pysnaptest` is a Python wrapper for the powerful [Insta](https://insta.rs/) library written in Rust. It brings the simplicity and performance of snapshot testing from Rust to Python, enabling developers to quickly and easily test complex outputs, including strings, JSON, and other serializable data.

Snapshot testing helps ensure that your code produces consistent outputs as you make changes. By capturing the output of your code and comparing it to a stored "snapshot," you can detect unintended changes with ease.

## Why pysnaptest?

Most of what sets `pysnaptest` apart from other Python snapshot libraries comes
from wrapping the mature Rust [Insta](https://insta.rs/) engine and adding a few
capabilities that peers don't offer out of the box:

- **Real insta `.snap` format, compatible with `cargo-insta review`.** You get
  insta's battle-tested snapshot format and its interactive diff/review tooling
  for free — no other Python tool reads and writes insta snapshots.
- **First-class DataFrame snapshots.** Snapshot pandas or polars `DataFrame`
  objects directly, serialized as CSV, JSON, parquet, or insta's binary format.
- **Mock-and-snapshot external calls.** `patch_json_snapshot` records the JSON
  result of a patched function as a snapshot, so tests that hit external APIs
  become deterministic without hand-written fixtures.
- **Insta redaction selectors** for scrubbing nondeterministic fields (ids,
  timestamps) before comparison.

It also keeps the basics other tools give you:

- **Fast and Lightweight**: Leverages Rust's high performance through the Insta library.
- **Simple Integration**: Easy-to-use Python API for snapshot testing.
- **Human-Readable Snapshots**: Snapshots are stored in a clean, readable format.
- **Automatic Snapshot Updates**: Conveniently update snapshots when intended changes are made.
- **CI-Friendly**: Great for continuous integration workflows.

## Installation

You can install `pysnaptest` via pip:

```bash
pip install pysnaptest
```

## Usage

The `snapshot` decorator makes it easy to capture the return value of a test
function:

```python
from pysnaptest import snapshot

@snapshot
def test_basic():
    return {"hello": "world"}
```

You can also assert snapshots directly without using a decorator:

```python
from pysnaptest import assert_json_snapshot

def test_direct():
    data = {"hello": "world"}
    assert_json_snapshot(data)
```

For tests that call external APIs you can patch the function and snapshot its
return value:

```python
from pysnaptest import patch_json_snapshot, snapshot
from my_project.main import use_http_request

@patch_json_snapshot("my_project.main.http_request")
@snapshot
def test_use_http_request():
    return use_http_request()
```

### Which API do I use?

All three entry points write the same insta snapshots — pick based on how your
test is shaped:

- **`@snapshot`** — when the thing you want to snapshot is a test function's
  return value. It auto-detects the type (dict/list → JSON, `bytes` → binary,
  pandas/polars → DataFrame, everything else → text) and works on `async`
  tests too.
- **`assert_json_snapshot` / `assert_snapshot` / `assert_csv_snapshot` /
  `assert_binary_snapshot` / `assert_dataframe_snapshot`** — when you want to
  assert a value mid-test, or need explicit control over the format, path, name,
  or redactions.
- **`patch_json_snapshot` / `mock_json_snapshot`** — when a function calls out to
  an external dependency (HTTP, DB) and you want to snapshot that call's JSON
  result instead of mocking it by hand.

## Updating Snapshots

If the output changes intentionally, you can review and update snapshots in two
ways: with the built-in, cargo-free workflow (recommended for Python projects)
or with `cargo-insta review`.

### Reviewing without cargo (recommended)

`pysnaptest` ships a pytest plugin and a small CLI so you can create, update, and
accept snapshots without installing any Rust tooling. insta itself does the work
of diffing and writing snapshots; the plugin only selects the update mode.

Update snapshots in place while running your tests (they pass on update, just
like other Python snapshot tools):

```bash
pytest --snapshot-update
```

Prefer to inspect changes and accept them yourself? Record pending `*.snap.new`
files instead, then review them the way `cargo insta review` does — one snapshot
at a time, showing insta's own diff and prompting to accept, reject, or skip:

```bash
pytest --snapshot-new          # record changed snapshots as pending files
pysnaptest review              # interactively review each pending snapshot
```

You can also act on all pending snapshots at once:

```bash
pysnaptest pending             # list pending snapshots with their diffs
pysnaptest accept              # accept every pending snapshot
pysnaptest reject              # discard every pending snapshot
```

The diffs shown by `review` and `pending` are rendered by insta itself, so they
match exactly what you see from a failing assertion or `cargo insta review`.

You can also drive this from Python:

```python
from pysnaptest.review import find_pending_snapshots, accept_pending_snapshot

for pending in find_pending_snapshots():
    accept_pending_snapshot(pending)
```

Set `INSTA_WORKSPACE_ROOT` so both the plugin and the CLI agree on where
snapshots live (see the example project's `pytest.ini`).

### Reviewing with `cargo-insta`

You can also use the [`cargo-insta`](https://insta.rs/) reviewer, which is
distributed via Rust's package manager `cargo`.

1. Install Rust using [rustup](https://rustup.rs/) if it is not already present.
2. Install the CLI with:

```bash
cargo install cargo-insta
```

With the binary on your `PATH` you can review snapshots using:

```bash
cargo-insta review
```

This command presents an interactive diff viewer where you can approve or reject
changes. Snapshot files are saved under a `snapshots` directory next to each test
module. Set the environment variable `INSTA_WORKSPACE_ROOT` when running your
tests so both the library and `cargo-insta` know where snapshots are stored. In
the example project the tests are inside a `tests` folder, so `pytest.ini` sets
`INSTA_WORKSPACE_ROOT=tests`, allowing the CLI to find
`examples/my_project/tests/snapshots`.


## Examples

To help you get started, we’ve included a collection of examples in the `examples` folder. These examples demonstrate how to use `pysnaptest` in projects and cover common use cases like snapshotting strings, JSON, and other data structures.

To try them out:

```bash
cd examples/my_project
pytest
```

Feel free to explore, modify, and build upon these examples for your own projects!

## Contributing

We welcome contributions to `pysnaptest`! To get started:

1. Fork the repository.
2. Create a new branch for your feature or fix.
3. Submit a pull request with a clear description of your changes.

## License

`pysnaptest` is licensed under the Apache License, Version 2.0. See the [LICENSE](LICENSE) file for details.

## Acknowledgments

This library is inspired by and built upon the excellent [Insta](https://insta.rs/) library. A big thank you to the Insta team for creating such a fantastic tool!
