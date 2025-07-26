# PySnapTest

`pysnaptest` is a Python wrapper for the powerful [Insta](https://insta.rs/) library written in Rust. It brings the simplicity and performance of snapshot testing from Rust to Python, enabling developers to quickly and easily test complex outputs, including strings, JSON, and other serializable data.

Snapshot testing helps ensure that your code produces consistent outputs as you make changes. By capturing the output of your code and comparing it to a stored "snapshot," you can detect unintended changes with ease.

## Features

- **Fast and Lightweight**: Leverages Rust's high performance through the Insta library.
- **Simple Integration**: Easy-to-use Python API for snapshot testing.
- **Human-Readable Snapshots**: Snapshots are stored in a clean, readable format.
- **Flexible Matchers**: Supports testing strings, JSON, and other data structures.
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

## Updating Snapshots

If the output changes intentionally, you can review and update snapshots using
`cargo-insta review`. The tool used for this, `cargo-insta`, is distributed via
Rust's package manager `cargo`.

### Installing `cargo-insta`

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

## Benchmarks

Below are some preliminary benchmark results comparing `pysnaptest` to
[`syrupy`](https://github.com/tophat/syrupy). The numbers were generated using
`pytest-benchmark` and are provided to give a general idea of runtime
performance.

```
------------------------------------------------------------------------------------------------------- benchmark: 4 tests -------------------------------------------------------------------------------------------------------
Name (time in us)                            Min                     Max                    Mean                StdDev                  Median                   IQR            Outliers         OPS            Rounds  Iterations
----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
test_pysnaptest_benchmark[10]           144.2080 (1.0)          224.8750 (1.0)          151.9010 (1.0)         16.6605 (1.0)          146.6665 (1.0)          5.1665 (1.0)           2;3  6,583.2353 (1.0)          24           1
test_syrupy_benchmark[10]               171.2920 (1.19)         242.0420 (1.08)         184.7244 (1.22)        21.8456 (1.31)         175.3750 (1.20)         9.9587 (1.93)          2;2  5,413.4703 (0.82)         13           1
test_pysnaptest_benchmark[1000]     266,551.1670 (>1000.0)  269,829.4590 (>1000.0)  268,094.9420 (>1000.0)  1,287.9046 (77.30)    267,992.9170 (>1000.0)  1,995.2603 (386.19)        2;0      3.7300 (0.00)          5           1
test_syrupy_benchmark[1000]         774,865.1250 (>1000.0)  779,754.2500 (>1000.0)  777,922.5250 (>1000.0)  1,949.4904 (117.01)   778,594.3330 (>1000.0)  2,682.1555 (519.14)        1;0      1.2855 (0.00)          5           1
----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------

Legend:
  Outliers: 1 Standard Deviation from Mean; 1.5 IQR (InterQuartile Range) from 1st Quartile and 3rd Quartile.
  OPS: Operations Per Second, computed as 1 / Mean
```

If the `syrupy` benchmarks above are not executed optimally, contributions are
very welcome to help benchmark more accurately.

## Contributing

We welcome contributions to `pysnaptest`! To get started:

1. Fork the repository.
2. Create a new branch for your feature or fix.
3. Submit a pull request with a clear description of your changes.

## License

`pysnaptest` is licensed under the Apache License, Version 2.0. See the [LICENSE](LICENSE) file for details.

## Acknowledgments

This library is inspired by and built upon the excellent [Insta](https://insta.rs/) library. A big thank you to the Insta team for creating such a fantastic tool!
