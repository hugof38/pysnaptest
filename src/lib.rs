#![deny(clippy::unwrap_used)]

use std::collections::{BTreeMap, HashMap};
use std::env::VarError;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::{self, FromStr};
use std::sync::{Mutex, MutexGuard};
use std::{env, path::Path};

use csv::ReaderBuilder;

use insta::assert_json_snapshot as assert_json_snapshot_macro;
use insta::internals::{Redaction, SnapshotContents};
use insta::Snapshot;
use insta::{rounded_redaction, sorted_redaction};
use once_cell::sync::Lazy;
use pyo3::types::{PyAnyMethods, PyDict, PyTuple};
use pyo3::{
    exceptions::PyValueError,
    pyclass, pyfunction, pymethods, pymodule,
    types::{PyModule, PyModuleMethods},
    wrap_pyfunction, Bound, PyAny, PyErr, PyResult,
};
use pyo3::{FromPyObject, Py, PyObject, Python};

const PYSNAPSHOT_SUFFIX: &str = "pysnap";

static TEST_NAME_COUNTERS: Lazy<Mutex<BTreeMap<String, usize>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug)]
struct Description {
    test_file_path: String,
}

impl Description {
    pub fn new(test_file_path: String) -> Self {
        Self { test_file_path }
    }
}

impl From<Description> for String {
    fn from(val: Description) -> Self {
        format!("Test File Path: {}", val.test_file_path)
    }
}

#[derive(Debug)]
struct PytestInfo {
    test_path: String,
    test_name: String,
}

#[derive(Debug)]
enum Error {
    CouldNotSplit(String),
    InvalidEnvVar(VarError),
    NoTestFile,
}

impl From<Error> for PyErr {
    fn from(value: Error) -> Self {
        match value {
            Error::CouldNotSplit(s) => PyValueError::new_err(format!(
                "Expected '::' to be in PYTEST_CURRENT_TEST string ({s})"
            )),
            Error::InvalidEnvVar(ve) => match ve {
                VarError::NotPresent => PyValueError::new_err("PYTEST_CURRENT_TEST is not set"),
                VarError::NotUnicode(os_string) => PyValueError::new_err(format!(
                    "PYTEST_CURRENT_TEST is not a valid unicode string: {os_string:#?}"
                )),
            },
            Error::NoTestFile => PyValueError::new_err("No test file found"),
        }
    }
}

impl PytestInfo {
    pub fn from_env() -> Result<Self, Error> {
        let pytest_str = env::var("PYTEST_CURRENT_TEST").map_err(Error::InvalidEnvVar)?;
        pytest_str.parse()
    }

    pub fn test_path(&self) -> Result<PathBuf, Error> {
        let path = self.test_path_raw();
        if path.exists() {
            Ok(path)
        } else if let Some(filename) = path.file_name() {
            let mut filepath = PathBuf::from("./");
            filepath.push(filename);
            Ok(filepath)
        } else {
            Err(Error::NoTestFile)
        }
    }

    pub fn test_path_raw(&self) -> PathBuf {
        Path::new(&self.test_path).to_path_buf()
    }
}

impl FromStr for PytestInfo {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (test_path, test_name_and_stage) = s
            .split_once("::")
            .ok_or(Error::CouldNotSplit(s.to_string()))?;

        let test_name = test_name_and_stage
            .split_once(" ")
            .map(|(tn, _stage)| tn)
            .unwrap_or(test_name_and_stage);

        Ok(PytestInfo {
            test_name: test_name.to_string(),
            test_path: test_path.to_string(),
        })
    }
}

#[pyclass(frozen)]
#[derive(Debug, Clone)]
struct SnapshotInfo {
    snapshot_folder: PathBuf,
    snapshot_name: String,
    relative_test_file_path: Option<String>,
    allow_duplicates: bool,
}

impl TryFrom<PytestInfo> for SnapshotInfo {
    type Error = PyErr;
    fn try_from(value: PytestInfo) -> Result<Self, Self::Error> {
        let test_file_dir = value
            .test_path()?
            .canonicalize()?
            .parent()
            .ok_or_else(|| {
                PyValueError::new_err(format!(
                    "Invalid test_path: {:?}, not yielding a parent directory",
                    value.test_path_raw()
                ))
            })?
            .join("snapshots");

        let test_name = &value.test_name;
        let test_path = value.test_path_raw();
        let file_name = test_path.file_stem().and_then(|s| s.to_str());

        let name = if let Some(f) = file_name {
            format!("{f}_{test_name}")
        } else {
            test_name.to_string()
        };
        Ok(Self {
            snapshot_folder: test_file_dir,
            snapshot_name: name,
            relative_test_file_path: Some(value.test_path()?.to_string_lossy().to_string()),
            allow_duplicates: false,
        })
    }
}

#[pymethods]
impl SnapshotInfo {
    #[staticmethod]
    #[pyo3(signature = (snapshot_path_override = None, snapshot_name_override = None, allow_duplicates = false))]
    fn from_pytest(
        snapshot_path_override: Option<PathBuf>,
        snapshot_name_override: Option<String>,
        allow_duplicates: bool,
    ) -> PyResult<Self> {
        Ok(
            if let (Some(snapshot_folder), Some(snapshot_name)) = (
                snapshot_path_override.clone(),
                snapshot_name_override.clone(),
            ) {
                Self {
                    snapshot_folder,
                    snapshot_name,
                    relative_test_file_path: None,
                    allow_duplicates,
                }
            } else {
                let pytest_info: SnapshotInfo = PytestInfo::from_env()?.try_into()?;
                Self {
                    snapshot_folder: snapshot_path_override.unwrap_or(pytest_info.snapshot_folder),
                    snapshot_name: snapshot_name_override.map_or(pytest_info.snapshot_name, |v| {
                        v.split('-').next().map_or(v.clone(), |s| s.to_string())
                    }),
                    relative_test_file_path: pytest_info.relative_test_file_path,
                    allow_duplicates,
                }
            },
        )
    }

    pub fn snapshot_folder(&self) -> &PathBuf {
        &self.snapshot_folder
    }

    pub fn last_snapshot_name(&self) -> String {
        let test_idx = Self::counters()
            .get(&self.snapshot_name)
            .cloned()
            .unwrap_or(1);
        self.snapshot_name_with_idx(test_idx)
    }

    pub fn next_snapshot_name(&self) -> String {
        let test_idx = Self::counters()
            .get(&self.snapshot_name)
            .cloned()
            .unwrap_or(0)
            + 1;
        self.snapshot_name_with_idx(test_idx)
    }

    pub fn last_snapshot_path(&self, module_path: Option<String>) -> PyResult<PathBuf> {
        let module_path = module_path
            .unwrap_or(module_path!().to_string())
            .replace("::", "__");
        Ok(self.snapshot_folder.join(format!(
            "{module_path}__{}@pysnap.snap",
            self.last_snapshot_name()
        )))
    }

    pub fn next_snapshot_path(&self, module_path: Option<String>) -> PyResult<PathBuf> {
        let module_path = module_path
            .unwrap_or(module_path!().to_string())
            .replace("::", "__");
        Ok(self.snapshot_folder.join(format!(
            "{module_path}__{}@pysnap.snap",
            self.next_snapshot_name()
        )))
    }
}

impl SnapshotInfo {
    fn counters<'a>() -> MutexGuard<'a, BTreeMap<String, usize>> {
        TEST_NAME_COUNTERS.lock().unwrap_or_else(|x| x.into_inner())
    }

    fn snapshot_name_with_idx(&self, test_idx: usize) -> String {
        if test_idx == 1 || test_idx == 0 {
            self.snapshot_name.to_string()
        } else {
            format!("{}-{}", self.snapshot_name, test_idx)
        }
    }

    fn snapshot_name(&self) -> String {
        let mut c = Self::counters();
        let mut test_idx = c.get(&self.snapshot_name).cloned().unwrap_or(0);
        if !self.allow_duplicates {
            test_idx += 1;
            c.insert(self.snapshot_name.clone(), test_idx);
        }

        self.snapshot_name_with_idx(test_idx)
    }
}

impl TryInto<insta::Settings> for &SnapshotInfo {
    type Error = PyErr;

    fn try_into(self) -> PyResult<insta::Settings> {
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path(self.snapshot_folder());
        settings.set_snapshot_suffix(PYSNAPSHOT_SUFFIX);
        if let Some(relative_test_file_path) = &self.relative_test_file_path {
            settings.set_description(Description::new(relative_test_file_path.clone()));
        }
        settings.set_omit_expression(true);
        Ok(settings)
    }
}

#[derive(Debug)]
pub enum RedactionType {
    Sorted,
    Rounded(usize),
    Standard(String),
}

impl<'source> FromPyObject<'source> for RedactionType {
    #[inline]
    fn extract_bound(ob: &Bound<'source, PyAny>) -> PyResult<Self> {
        if ob.is_none() {
            Ok(RedactionType::Sorted)
        } else if let Ok(decimals) = ob.extract::<usize>() {
            Ok(RedactionType::Rounded(decimals))
        } else if let Ok(redaction) = ob.extract::<String>() {
            Ok(RedactionType::Standard(redaction))
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
                "Unable to extract RedactionType",
            ))
        }
    }
}

impl From<RedactionType> for Redaction {
    fn from(value: RedactionType) -> Self {
        match value {
            RedactionType::Sorted => sorted_redaction(),
            RedactionType::Rounded(decimals) => rounded_redaction(decimals),
            RedactionType::Standard(redaction) => redaction.into(),
        }
    }
}

#[pyclass(unsendable)]
#[derive(Debug)]
pub struct PySnapshot(Snapshot);

#[pymethods]
impl PySnapshot {
    #[staticmethod]
    pub fn from_file(p: PathBuf) -> PyResult<Self> {
        Ok(Self(Snapshot::from_file(&p).map_err(|e| {
            PyValueError::new_err(format!("Unable to load snapshot from {p:?}, details: {e}",))
        })?))
    }

    pub fn contents(&self) -> PyResult<Vec<u8>> {
        Ok(match self.0.contents() {
            SnapshotContents::Text(text_snapshot_contents) => {
                text_snapshot_contents.to_string().as_bytes().to_vec()
            }
            SnapshotContents::Binary(items) => items.deref().to_owned(),
        })
    }
}

#[pyfunction]
#[pyo3(signature = (test_info, result, redactions=None))]
fn assert_json_snapshot(
    test_info: &SnapshotInfo,
    result: &Bound<'_, PyAny>,
    redactions: Option<HashMap<String, RedactionType>>,
) -> PyResult<()> {
    let res: serde_json::Value = pythonize::depythonize(result)?;
    let snapshot_name = test_info.snapshot_name();
    let mut settings: insta::Settings = test_info.try_into()?;

    for (selector, redaction) in redactions.unwrap_or_default() {
        settings.add_redaction(selector.as_str(), redaction)
    }

    settings.bind(|| {
        insta::assert_json_snapshot!(snapshot_name, res);
    });
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (test_info, result, redactions=None))]
fn assert_csv_snapshot(
    test_info: &SnapshotInfo,
    result: &str,
    redactions: Option<HashMap<String, RedactionType>>,
) -> PyResult<()> {
    let mut rdr = ReaderBuilder::new().from_reader(result.as_bytes());
    let columns: Vec<Vec<serde_json::Value>> = vec![rdr
        .headers()
        .expect("Expects csv with headers")
        .into_iter()
        .map(|h| h.into())
        .collect()];
    let records = rdr
        .into_deserialize()
        .collect::<Result<Vec<Vec<serde_json::Value>>, _>>()
        .expect("Failed to parse csv records");
    let res: Vec<Vec<serde_json::Value>> = columns.into_iter().chain(records).collect();

    let snapshot_name = test_info.snapshot_name();
    let mut settings: insta::Settings = test_info.try_into()?;

    for (selector, redaction) in redactions.unwrap_or_default() {
        settings.add_redaction(selector.as_str(), redaction)
    }

    settings.bind(|| {
        insta::assert_csv_snapshot!(snapshot_name, res);
    });
    Ok(())
}

#[pyfunction]
fn assert_binary_snapshot(
    test_info: &SnapshotInfo,
    extension: &str,
    result: Vec<u8>,
) -> PyResult<()> {
    let snapshot_name = test_info.snapshot_name();
    let settings: insta::Settings = test_info.try_into()?;
    settings.bind(|| {
        insta::assert_binary_snapshot!(format!("{snapshot_name}.{extension}").as_str(), result);
    });
    Ok(())
}

#[pyfunction]
fn assert_snapshot(test_info: &SnapshotInfo, result: &Bound<'_, PyAny>) -> PyResult<()> {
    let snapshot_name = test_info.snapshot_name();
    let settings: insta::Settings = test_info.try_into()?;
    settings.bind(|| {
        insta::assert_snapshot!(snapshot_name, result);
    });
    Ok(())
}

macro_rules! snapshot_fn_auto {
    ($f:expr $(, $arg:ident )* ; serialize_macro = $serialize_macro:ident ; result_from_str=$result_from_str:expr) => {{
        let f = $f;
        let name = stringify!($f);
        let module_path = module_path!();

        move |$( $arg ),+, info: &SnapshotInfo, redactions: Option<HashMap<String, RedactionType>>, record: bool| -> Result<_, anyhow::Error> {
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
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?
                    .contents()
                {
                    SnapshotContents::Text(content) => {
                        Ok(($result_from_str)(content.to_string())?)
                    },
                    SnapshotContents::Binary(_) => Err(anyhow::anyhow!(
                        "Snapshot at {:?} is binary, which is not supported for deserialization",
                        snapshot_path
                    )),
                }
            }
        }
    }};
}

#[macro_export]
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
struct PyMockWrapper {
    f: Box<
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
    snapshot_info: SnapshotInfo,
    record: bool,
}

#[pymethods]
impl PyMockWrapper {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        (self.f)(args, kwargs, &self.snapshot_info, None, self.record)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }
}

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
fn mock_json_snapshot(
    py_fn: PyObject,
    snapshot_info: SnapshotInfo,
    record: bool,
) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        let callable = Py::new(
            py,
            PyMockWrapper {
                f: Box::new(wrap_py_fn_snapshot_json(py_fn)),
                snapshot_info,
                record,
            },
        )?;
        Ok(callable.into())
    })
}

#[pymodule]
#[pyo3(name = "_pysnaptest")]
fn pysnaptest(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SnapshotInfo>()?;

    m.add_function(wrap_pyfunction!(assert_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(assert_binary_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(assert_json_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(assert_csv_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(mock_json_snapshot, m)?)?;
    m.add_class::<PySnapshot>()?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use std::{
        cell::Cell,
        ffi::CString,
        path::{Path, PathBuf},
        rc::Rc,
    };

    use pyo3::{types::PyDict, IntoPyObject};

    use super::*;

    use crate::{Error, PytestInfo, SnapshotInfo};

    #[test]
    fn test_into_pyinfo_happy_path() {
        let s = "tests/a/b/test_thing.py::test_a (call)";
        let pti: Result<PytestInfo, Error> = s.parse();
        insta::assert_debug_snapshot!(pti)
    }

    #[test]
    fn test_into_pyinfo_no_trailer() {
        let s = "tests/a/b/test_thing.py::test_a";
        let pti: Result<PytestInfo, Error> = s.parse();
        insta::assert_debug_snapshot!(pti)
    }

    #[test]
    fn test_into_pyinfo_failure_case() {
        let s = "tests/a/b/test_thing.py";
        let pti: Result<PytestInfo, Error> = s.parse();
        insta::assert_debug_snapshot!(pti)
    }

    #[test]
    fn test_snapshot_info_overrides_from_pytest() {
        let snapshot_info = SnapshotInfo::from_pytest(
            Some("folder_path_override".into()),
            Some("snapshot_name_override".into()),
            false,
        )
        .unwrap();
        insta::assert_debug_snapshot!(snapshot_info);
        insta::assert_snapshot!(snapshot_info.snapshot_name(), @"snapshot_name_override");
        insta::assert_snapshot!(snapshot_info.last_snapshot_name(), @"snapshot_name_override");
        insta::assert_snapshot!(snapshot_info.next_snapshot_name(), @"snapshot_name_override-2");
        insta::assert_snapshot!(snapshot_info.snapshot_name(), @"snapshot_name_override-2");
        insta::assert_snapshot!(snapshot_info.last_snapshot_name(), @"snapshot_name_override-2");
        insta::assert_snapshot!(snapshot_info.next_snapshot_name(), @"snapshot_name_override-3");
    }

    fn snapshot_folder_path() -> PathBuf {
        // This env var points to the root of your crate during cargo test/build
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        crate_root.join("src").join("snapshots")
    }

    #[test]
    fn test_snapshot_json_or_mock_creates_and_reads_snapshot() -> Result<(), anyhow::Error> {
        let input_1 = 4;

        // Shared counter to track how many times the function is called
        let call_count = Rc::new(Cell::new(0));
        let call_count_clone = Rc::clone(&call_count);

        let f = |i| {
            call_count_clone.set(call_count_clone.get() + 1);
            Ok::<_, anyhow::Error>(i * 2)
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
    fn test_create_mocked_pyfn_creates_and_reads_snapshot() -> Result<(), anyhow::Error> {
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
            let wrapper_obj = mock_json_snapshot(py_fn.clone_ref(py), snapshot_info.clone(), true)?;
            let wrapper = wrapper_obj.bind(py);

            let args = PyTuple::new(py, 7.into_pyobject(py))?;

            let result1: Bound<'_, PyDict> = wrapper.call1(args)?.extract()?;
            assert_eq!(result1.get_item("result").unwrap().extract::<i32>()?, 70);
            assert_eq!(result1.get_item("calls").unwrap().extract::<i32>()?, 1);

            let wrapper_obj = mock_json_snapshot(py_fn, snapshot_info.clone(), false)?;
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
