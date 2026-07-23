//! Rust primitives backing the Python mock layer.
//!
//! The orchestration for function mocks (deciding whether to actually call the
//! wrapped function, and Pydantic/dataclass normalization via
//! `pysnaptest.to_jsonable`) lives in Python (`pysnaptest.mocks`). But the
//! fiddly, easy-to-get-wrong bookkeeping around snapshot naming -- scoping a
//! mock's name, peeking its response path *before* ticking the shared
//! duplicate counter, and deciding record-vs-replay from that path -- is done
//! once, here, in `prepare_mock_call`, by composing the existing `SnapshotInfo`
//! naming methods rather than duplicating their logic.
//!
//! This module exposes three thin functions to Python:
//!
//! * `prepare_mock_call` scopes the snapshot name, writes the request
//!   snapshot (reusing `crate::bind_json_snapshot`) and returns the response
//!   snapshot's name/path/record-decision,
//! * `assert_json_snapshot_named` writes a JSON snapshot under an explicit
//!   name (also reusing `crate::bind_json_snapshot`), used for the response
//!   snapshot once the wrapped function has actually been called, and
//! * `read_json_snapshot` loads a recorded snapshot back into Python (reusing
//!   insta's own file parser), used to replay a response without calling the
//!   wrapped function.
//!
//! All three live in this module so the on-disk `pysnaptest__mocks__*` filename
//! prefix (derived from `module_path!()` at the `insta::assert_json_snapshot!`
//! call site) is preserved.

use std::collections::HashMap;
use std::path::PathBuf;

use insta::internals::SnapshotContents;
use insta::Snapshot;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::{RedactionType, SnapshotInfo};

/// Scope `test_info` to a mock of `suffix`, write its request snapshot, and
/// report where/whether the response should be recorded.
///
/// Returns `(name, response_path, do_record)`: `name` is the response
/// snapshot's assigned name (to pass back into `assert_json_snapshot_named`
/// once the wrapped function has run), `response_path` is its on-disk path,
/// and `do_record` is `true` when the wrapped function should actually be
/// called (either `record` was requested, or no response snapshot exists yet).
///
/// The response path is deliberately computed via `next_snapshot_path` (a
/// peek) *before* `snapshot_name` (which ticks the shared duplicate counter);
/// getting that ordering right is exactly the kind of bookkeeping this
/// function exists to own on Python's behalf.
#[pyfunction]
#[pyo3(signature = (test_info, suffix, request, record, redactions=None))]
pub fn prepare_mock_call(
    test_info: &SnapshotInfo,
    suffix: &str,
    request: &Bound<'_, PyAny>,
    record: bool,
    redactions: Option<HashMap<String, RedactionType>>,
) -> PyResult<(String, PathBuf, bool)> {
    let finfo = test_info.with_name_suffix(suffix);
    let response_path = finfo.next_snapshot_path(Some(module_path!().to_string()))?;
    let name = finfo.snapshot_name();

    let request_json: serde_json::Value = pythonize::depythonize(request)?;
    crate::bind_json_snapshot!(
        test_info,
        request_json,
        format!("{name}-request"),
        redactions
    )?;

    let do_record = record || !response_path.exists();
    Ok((name, response_path, do_record))
}

/// Assert a JSON snapshot under an explicit `name`, without ticking the
/// duplicate counter.
///
/// This mirrors `crate::assert_json_snapshot` but takes the snapshot name
/// directly, which lets the Python mock layer write a mocked call's response
/// snapshot under the name reserved by `prepare_mock_call`. `result` is
/// expected to already be JSON-native (the Python side normalizes rich
/// objects with `pysnaptest.to_jsonable` first).
#[pyfunction]
#[pyo3(signature = (test_info, result, name, redactions=None))]
pub fn assert_json_snapshot_named(
    test_info: &SnapshotInfo,
    result: &Bound<'_, PyAny>,
    name: String,
    redactions: Option<HashMap<String, RedactionType>>,
) -> PyResult<()> {
    let res: serde_json::Value = pythonize::depythonize(result)?;
    crate::bind_json_snapshot!(test_info, res, name, redactions)
}

/// Read a previously recorded JSON snapshot file and return its parsed value.
///
/// Used by the Python mock layer during replay: the recorded response is loaded
/// through insta (so the snapshot file format is handled in one place) and
/// converted back into native Python objects.
#[pyfunction]
pub fn read_json_snapshot(snapshot_path: PathBuf) -> PyResult<PyObject> {
    let snapshot = Snapshot::from_file(&snapshot_path).map_err(|e| {
        PyValueError::new_err(format!(
            "Unable to load snapshot from {snapshot_path:?}: {e}"
        ))
    })?;
    match snapshot.contents() {
        SnapshotContents::Text(content) => Python::with_gil(|py| {
            let value: serde_json::Value =
                serde_json::from_str(&content.to_string()).map_err(|e| {
                    PyValueError::new_err(format!(
                        "Invalid JSON in snapshot {snapshot_path:?}: {e}"
                    ))
                })?;
            let obj = pythonize::pythonize(py, &value).map_err(|e| {
                PyValueError::new_err(format!("Failed to convert snapshot to Python: {e}"))
            })?;
            Ok(obj.into())
        }),
        SnapshotContents::Binary(_) => Err(PyValueError::new_err(format!(
            "Snapshot at {snapshot_path:?} is binary, which is not supported for mock replay"
        ))),
    }
}
