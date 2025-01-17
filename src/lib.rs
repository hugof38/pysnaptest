use std::collections::HashMap;
use std::env::VarError;
use std::path::PathBuf;
use std::str::{self, FromStr};
use std::{env, path::Path};

use csv::ReaderBuilder;
use pyo3::{
    exceptions::PyValueError,
    pyclass, pyfunction, pymethods, pymodule,
    types::{PyModule, PyModuleMethods},
    wrap_pyfunction, Bound, PyAny, PyErr, PyResult,
};

const PYSNAPSHOT_SUFFIX: &str = "pysnap";

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
struct TestInfo {
    pytest_info: PytestInfo,

    snapshot_path_override: Option<PathBuf>,
    snapshot_name_override: Option<String>,
}

#[pymethods]
impl TestInfo {
    #[staticmethod]
    #[pyo3(signature = (snapshot_path_override = None, snapshot_name_override = None))]
    fn from_pytest(
        snapshot_path_override: Option<PathBuf>,
        snapshot_name_override: Option<String>,
    ) -> PyResult<Self> {
        let pytest_info = PytestInfo::from_env()?;
        Ok(TestInfo {
            pytest_info,
            snapshot_path_override,
            snapshot_name_override,
        })
    }

    fn snapshot_path(&self) -> PyResult<PathBuf> {
        if let Some(snapshot_path) = self.snapshot_path_override.clone() {
            return Ok(snapshot_path);
        }

        let mut test_file_dir = self
            .pytest_info
            .test_path()?
            .canonicalize()?
            .parent()
            .ok_or_else(|| {
                PyValueError::new_err(format!(
                    "Invalid test_path: {:?}, not yielding a parent directory",
                    self.pytest_info.test_path_raw()
                ))
            })?
            .to_path_buf();
        test_file_dir.push("snapshots");

        Ok(test_file_dir)
    }

    fn snapshot_name(&self) -> String {
        if let Some(sno) = self.snapshot_name_override.as_ref() {
            return sno.clone();
        }

        let test_name = &self.pytest_info.test_name;
        let test_path = self.pytest_info.test_path_raw();
        let file_name = test_path.file_stem().and_then(|s| s.to_str());
        if let Some(f) = file_name {
            format!("{f}_{test_name}")
        } else {
            test_name.to_string()
        }
    }
}

impl TryInto<insta::Settings> for &TestInfo {
    type Error = PyErr;

    fn try_into(self) -> PyResult<insta::Settings> {
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path(self.snapshot_path()?);
        settings.set_snapshot_suffix(PYSNAPSHOT_SUFFIX);
        settings.set_description(Description::new(
            self.pytest_info.test_path()?.to_string_lossy().to_string(),
        ));
        settings.set_omit_expression(true);
        Ok(settings)
    }
}

#[pyfunction]
#[pyo3(signature = (test_info, result, redactions=None))]
fn assert_json_snapshot(
    test_info: &TestInfo,
    result: &Bound<'_, PyAny>,
    redactions: Option<HashMap<String, String>>,
) -> PyResult<()> {
    let res: serde_json::Value = pythonize::depythonize(result).unwrap();
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
    test_info: &TestInfo,
    result: &str,
    redactions: Option<HashMap<String, String>>,
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
fn assert_snapshot(test_info: &TestInfo, result: &Bound<'_, PyAny>) -> PyResult<()> {
    let snapshot_name = test_info.snapshot_name();
    let settings: insta::Settings = test_info.try_into()?;
    settings.bind(|| {
        insta::assert_snapshot!(snapshot_name, result);
    });
    Ok(())
}

#[pymodule]
#[pyo3(name = "_pysnaptest")]
fn pysnaptest(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<TestInfo>()?;

    m.add_function(wrap_pyfunction!(assert_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(assert_json_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(assert_csv_snapshot, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use insta::assert_debug_snapshot;

    use crate::{Error, PytestInfo};

    #[test]
    fn test_into_pyinfo_happy_path() {
        let s = "tests/a/b/test_thing.py::test_a (call)";
        let pti: Result<PytestInfo, Error> = s.parse();
        assert_debug_snapshot!(pti)
    }

    #[test]
    fn test_into_pyinfo_no_trailer() {
        let s = "tests/a/b/test_thing.py::test_a";
        let pti: Result<PytestInfo, Error> = s.parse();
        assert_debug_snapshot!(pti)
    }

    #[test]
    fn test_into_pyinfo_failure_case() {
        let s = "tests/a/b/test_thing.py";
        let pti: Result<PytestInfo, Error> = s.parse();
        assert_debug_snapshot!(pti)
    }
}
