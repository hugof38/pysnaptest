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

## Updating Snapshots

If the output changes intentionally, you can review and update snapshots using the `cargo-insta review` command. This provides an interactive workflow to inspect changes and accept or reject updates.

### Running Snapshot Review

You can install `cargo-insta` binaries from [cargo-insta](https://github.com/mitsuhiko/insta/tree/master/cargo-insta). After that you should be able to review snapshots with the following:
```bash
cargo-insta review
```

This command allows you to inspect differences and choose which snapshots to update.

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