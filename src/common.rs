use std::collections::BTreeMap;
use std::env::VarError;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::{self, FromStr};
use std::sync::{Mutex, MutexGuard};
use std::{env, path::Path};

use once_cell::sync::Lazy;

use pyo3::FromPyObject;
use pyo3::{
    exceptions::PyValueError,
    pyclass, pymethods, PyErr, PyResult, Bound, PyAny,
};

use insta::internals::{Redaction, SnapshotContents};
use insta::{rounded_redaction, sorted_redaction, Snapshot};
use pyo3::types::PyAnyMethods;

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
pub(crate) struct PytestInfo {
    test_path: String,
    test_name: String,
}

#[derive(Debug)]
pub(crate) enum Error {
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
pub struct SnapshotInfo {
    pub(crate) snapshot_folder: PathBuf,
    pub(crate) snapshot_name: String,
    pub(crate) relative_test_file_path: Option<String>,
    pub(crate) allow_duplicates: bool,
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

impl SnapshotInfo {
    pub(crate) fn counters<'a>() -> MutexGuard<'a, BTreeMap<String, usize>> {
        TEST_NAME_COUNTERS.lock().unwrap_or_else(|x| x.into_inner())
    }

    pub(crate) fn snapshot_name_with_idx(&self, test_idx: usize) -> String {
        if test_idx == 1 || test_idx == 0 {
            self.snapshot_name.to_string()
        } else {
            format!("{}-{}", self.snapshot_name, test_idx)
        }
    }

    pub(crate) fn snapshot_name(&self) -> String {
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

#[derive(Debug, Clone)]
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
            pyo3::exceptions::PyValueError::new_err(format!(
                "Unable to load snapshot from {p:?}, details: {e}",
            ))
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

#[cfg(test)]
mod tests {

    use super::*;

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

}
