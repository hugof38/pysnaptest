use std::collections::HashMap;

use insta::assert_json_snapshot as assert_json_snapshot_macro;
use insta::internals::SnapshotContents;
use insta::Snapshot;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use crate::errors::{SnapError, SnapResult};
use crate::{RedactionType, SnapshotInfo};

macro_rules! snapshot_fn_auto {
    ($f:expr $(, $arg:ident )* ; serialize_macro = $serialize_macro:ident ; result_from_str=$result_from_str:expr) => {{
        let f = $f;
        let name = stringify!($f);
        let module_path = module_path!();

        move |$( $arg ),+, info: &SnapshotInfo, redactions: Option<HashMap<String, RedactionType>>, record: bool| -> SnapResult<_> {
            let finfo = SnapshotInfo {
                snapshot_name: format!("{}_{}", info.snapshot_name, name),
                ..info.clone()
            };
            let snapshot_path = finfo.next_snapshot_path(Some(module_path.to_string()))?;
            let snapshot_name = finfo.snapshot_name();
            let mut settings: insta::Settings = (&finfo).try_into()?;

            for (selector, redaction) in redactions.unwrap_or_default() {
                settings.add_redaction(selector.as_str(), redaction);
            }

            // Serialize the input using the passed closure
            settings.bind(|| {
                $serialize_macro!(format!("{snapshot_name}-request"), ($( $arg ),+));
            });


            if record || !snapshot_path.exists() {
                let result = f($( $arg ),+)?;
                settings.bind(|| {
                    $serialize_macro!(snapshot_name, result);
                });
                Ok(result)
            } else {
                match Snapshot::from_file(&snapshot_path)
                    .map_err(SnapError::from)?
                    .contents()
                {
                    SnapshotContents::Text(content) => {
                        Ok(($result_from_str)(content.to_string())?)
                    },
                    SnapshotContents::Binary(_) => Err(SnapError::from(
                        format!(
                            "Snapshot at {:?} is binary, which is not supported for deserialization",
                            snapshot_path
                        ),
                    )),
                }
            }
        }
    }};
}

macro_rules! snapshot_fn_auto_json {
    ($f:expr $(, $arg:ident )* ; serialize_macro = $serialize_macro:ident ; result_from_str=$result_from_str:expr) => {
        snapshot_fn_auto!($f $(, $arg )* ; serialize_macro = $serialize_macro ; result_from_str=$result_from_str)
    };

    ($f:expr $(, $arg:ident )* ) => {
        snapshot_fn_auto_json!(
            $f,
            $( $arg ),+;
            serialize_macro=assert_json_snapshot_macro;
            result_from_str=|content: String| serde_json::from_str(&content)
        )
    };
}

macro_rules! assert_json_snapshot_depythonize {
    ($snapshot_name:expr, ($arg:expr, $kwargs:expr ) ) => {{
        // Create a tuple of depythonized values

        let rust_args = pythonize::depythonize::<serde_json::Value>($arg as &Bound<PyAny>)
            .expect(&format!("Failed to depythonize args {:?}", $arg));
        let rust_kwargs = Option::<&Bound<'_, PyDict>>::map($kwargs, |kw| {
            pythonize::depythonize::<serde_json::Value>(kw as &Bound<PyAny>)
                .expect(&format!("Failed to depythonize kwargs {:?}", kw))
        });
        let input_json = serde_json::json!({
            "args": rust_args,
            "kwargs": rust_kwargs.unwrap_or(serde_json::Value::Null)
        });

        assert_json_snapshot_macro!($snapshot_name, input_json);
    }};
    ($snapshot_name:expr, $arg:expr) => {{
        Python::with_gil(|py| {
            let bound: &pyo3::Bound<PyAny> = $arg.bind(py);
            let input_tuple = pythonize::depythonize::<serde_json::Value>(&bound)
                .expect(&format!("Failed to depythonize {:?}", $arg));
            assert_json_snapshot_macro!($snapshot_name, input_tuple);
        });
    }};
}

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
            ) -> SnapResult<Py<PyAny>>
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
) -> SnapResult<Py<PyAny>>
       + Send
       + Sync {
    move |args: &Bound<'_, PyTuple>,
          kwargs: Option<&Bound<'_, _>>,
          info: &SnapshotInfo,
          redactions: Option<HashMap<String, RedactionType>>,
          record: bool| {
        let py_fn_cloned = Python::with_gil(|py| py_fn.clone_ref(py));

        let call_fn = move |args: &Bound<'_, PyTuple>,
                            kwargs: Option<&Bound<'_, _>>|
              -> SnapResult<PyObject> {
            Python::with_gil(|py| py_fn_cloned.call(py, args, kwargs)).map_err(SnapError::from)
        };

        let wrapped_fn = snapshot_fn_auto_json!(
            call_fn, args, kwargs;
            serialize_macro=assert_json_snapshot_depythonize;
            result_from_str=|content: String| -> SnapResult<PyObject> {
                Python::with_gil(|py| {
                    let value: serde_json::Value = serde_json::from_str(&content)?;
                    let obj = pythonize::pythonize(py, &value).map_err(SnapError::from)?;
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{RedactionType, SnapError, SnapResult};
    use insta::assert_json_snapshot as assert_json_snapshot_macro;
    use insta::internals::SnapshotContents;
    use insta::Snapshot;
    use std::{
        cell::Cell,
        ffi::CString,
        path::{Path, PathBuf},
        rc::Rc,
    };

    use pyo3::{
        types::{PyAnyMethods, PyDict, PyModule, PyTuple},
        Bound, IntoPyObject, Py, PyAny, PyResult, Python,
    };

    use crate::{mock_json_snapshot, SnapshotInfo};

    fn snapshot_folder_path() -> PathBuf {
        // This env var points to the root of your crate during cargo test/build
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        crate_root.join("src").join("snapshots")
    }

    #[test]
    fn test_snapshot_json_or_mock_creates_and_reads_snapshot() -> SnapResult<()> {
        let input_1 = 4;

        // Shared counter to track how many times the function is called
        let call_count = Rc::new(Cell::new(0));
        let call_count_clone = Rc::clone(&call_count);

        let f = |i| {
            call_count_clone.set(call_count_clone.get() + 1);
            Ok::<_, SnapError>(i * 2)
        };

        let snaphot_folder_path = snapshot_folder_path();
        let snapshot_info = SnapshotInfo {
            snapshot_folder: snaphot_folder_path,
            snapshot_name: "test_create_snapshot_fn".to_string(),
            relative_test_file_path: None,
            allow_duplicates: true,
        };

        let snapshot_json_or_mock = snapshot_fn_auto_json!(f, x);

        // First run: record mode, should call the function
        let result_1: i32 = snapshot_json_or_mock(input_1, &snapshot_info, None, true)?;
        assert_eq!(result_1, 8);
        assert_eq!(
            call_count.get(),
            1,
            "Function should have been called once during recording"
        );

        // Second run: replay mode, should NOT call the function
        let result_2: i32 = snapshot_json_or_mock(input_1, &snapshot_info, None, false)?;
        assert_eq!(result_2, 8);
        assert_eq!(
            call_count.get(),
            1,
            "Function should NOT have been called again during replay"
        );

        Ok(())
    }

    #[test]
    fn test_create_mocked_pyfn_creates_and_reads_snapshot() -> SnapResult<()> {
        pyo3::prepare_freethreaded_python();
        let snapshot_info = SnapshotInfo {
            snapshot_name: "test_create_mocked_pyfn".to_string(),
            relative_test_file_path: None,
            allow_duplicates: true,
            snapshot_folder: snapshot_folder_path(),
        };

        Python::with_gil(|py| -> PyResult<()> {
            // Define a Python function with a mutable counter
            let code = r#"
counter = {"calls": 0}
def compute(x):
    counter["calls"] += 1
    return {"result": x * 10, "calls": counter["calls"]}
"#;

            let module = PyModule::from_code(
                py,
                CString::new(code)?.as_c_str(),
                CString::new("testmod.py")?.as_c_str(),
                CString::new("testmod")?.as_c_str(),
            )?;
            let py_fn: Py<PyAny> = module.getattr("compute")?.into_pyobject(py)?.into();

            // Wrap with snapshot function in RECORDING mode
            let wrapper_obj =
                mock_json_snapshot(py_fn.clone_ref(py), snapshot_info.clone(), true, None)?;
            let wrapper = wrapper_obj.bind(py);

            let args = PyTuple::new(py, 7.into_pyobject(py))?;

            let result1: Bound<'_, PyDict> = wrapper.call1(args)?.extract()?;
            assert_eq!(result1.get_item("result").unwrap().extract::<i32>()?, 70);
            assert_eq!(result1.get_item("calls").unwrap().extract::<i32>()?, 1);

            let wrapper_obj = mock_json_snapshot(py_fn, snapshot_info.clone(), false, None)?;
            let wrapper = wrapper_obj.bind(py);
            let args = PyTuple::new(py, 7.into_pyobject(py))?;

            let result2: Bound<'_, PyDict> = wrapper.call1(args)?.extract()?;
            assert_eq!(result2.get_item("result").unwrap().extract::<i32>()?, 70);
            assert_eq!(result2.get_item("calls").unwrap().extract::<i32>()?, 1);

            Ok(())
        })?;

        Ok(())
    }
}
