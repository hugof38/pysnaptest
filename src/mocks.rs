use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict, PyTuple};

use crate::{assert_json_snapshot_depythonize, snapshot_fn_auto_json, RedactionType, SnapshotInfo};

#[pyclass]
#[allow(clippy::type_complexity)]
pub struct PyMockWrapper {
    pub f: Box<
        dyn for<'a> Fn(
                &'a Bound<'_, PyTuple>,
                Option<&'a Bound<'_, PyDict>>,
                &'a SnapshotInfo,
                Option<HashMap<String, RedactionType>>,
                bool,
            ) -> Result<Py<PyAny>, anyhow::Error>
            + Send
            + Sync,
    >,
    pub snapshot_info: SnapshotInfo,
    pub record: bool,
    pub redactions: Option<HashMap<String, RedactionType>>,
}

#[pymethods]
impl PyMockWrapper {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        (self.f)(
            args,
            kwargs,
            &self.snapshot_info,
            self.redactions.clone(),
            self.record,
        )
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }
}

#[allow(clippy::type_complexity)]
fn wrap_py_fn_snapshot_json(
    py_fn: PyObject,
) -> impl for<'b> Fn(
    &'b Bound<'_, PyTuple>,
    Option<&'b Bound<'_, PyDict>>,
    &'b SnapshotInfo,
    Option<HashMap<String, RedactionType>>,
    bool,
) -> Result<Py<PyAny>, anyhow::Error>
       + Send
       + Sync {
    move |args: &Bound<'_, PyTuple>,
          kwargs: Option<&Bound<'_, _>>,
          info: &SnapshotInfo,
          redactions: Option<HashMap<String, RedactionType>>,
          record: bool| {
        let py_fn_cloned = Python::with_gil(|py| py_fn.clone_ref(py));

        let call_fn =
            move |args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, _>>| -> PyResult<PyObject> {
                Python::with_gil(|py| py_fn_cloned.call(py, args, kwargs))
            };

        let wrapped_fn = snapshot_fn_auto_json!(
            call_fn, args, kwargs;
            serialize_macro=assert_json_snapshot_depythonize;
            result_from_str=|content: String| -> PyResult<PyObject> {
                Python::with_gil(|py| {
                    let value: serde_json::Value = serde_json::from_str(&content)
                        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                    let obj = pythonize::pythonize(py, &value)?;
                    Ok(obj.into())
                })
            }
        );

        wrapped_fn(args, kwargs, info, redactions, record)
    }
}

#[pyfunction]
pub fn mock_json_snapshot(
    py_fn: PyObject,
    snapshot_info: SnapshotInfo,
    record: bool,
    redactions: Option<HashMap<String, RedactionType>>,
) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        let callable = Py::new(
            py,
            PyMockWrapper {
                f: Box::new(wrap_py_fn_snapshot_json(py_fn)),
                snapshot_info,
                record,
                redactions,
            },
        )?;
        Ok(callable.into())
    })
}
