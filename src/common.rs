use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::{self, FromStr};
use std::sync::{Mutex, MutexGuard};
use std::{env, path::Path};

use once_cell::sync::Lazy;

use pyo3::FromPyObject;
use pyo3::{exceptions::PyValueError, pyclass, pymethods, Bound, PyAny, PyErr, PyResult};

use crate::errors::PytestInfoError;

use insta::internals::{Redaction, SnapshotContents};
use insta::{rounded_redaction, sorted_redaction, Snapshot};
use pyo3::types::PyAnyMethods;

const PYSNAPSHOT_SUFFIX: &str = "pysnap";

/// Filename tail of every committed pysnaptest snapshot, e.g.
/// `foo@pysnap.snap`. insta composes snapshot files as `<name>@<suffix>.snap`,
/// so this is `@{PYSNAPSHOT_SUFFIX}.snap`. Exposed to Python
/// (`pysnaptest._pysnaptest.SNAPSHOT_SUFFIX`) so `review`/`unused` derive their
/// globs from the same source of truth.
pub const SNAPSHOT_FILE_SUFFIX: &str = "@pysnap.snap";

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

impl PytestInfo {
    pub fn from_env() -> Result<Self, PytestInfoError> {
        let pytest_str = env::var("PYTEST_CURRENT_TEST").map_err(PytestInfoError::InvalidEnvVar)?;
        pytest_str.parse()
    }

    pub fn test_path(&self) -> Result<PathBuf, PytestInfoError> {
        let path = self.test_path_raw();
        if path.exists() {
            Ok(path)
        } else if let Some(filename) = path.file_name() {
            let mut filepath = PathBuf::from("./");
            filepath.push(filename);
            Ok(filepath)
        } else {
            Err(PytestInfoError::NoTestFile)
        }
    }

    pub fn test_path_raw(&self) -> PathBuf {
        Path::new(&self.test_path).to_path_buf()
    }

    /// Resolve the absolute path to the running test's source file.
    ///
    /// The path component of `PYTEST_CURRENT_TEST` (exposed here via
    /// [`Self::test_path_raw`]) is always relative to pytest's *rootdir*, not to
    /// the process working directory. When pytest supplies the test file's
    /// absolute path (its plugin reads `item.path`, which pytest guarantees to
    /// be absolute) we use it directly so snapshot resolution is independent of
    /// the invocation directory. Only when no such path is available do we fall
    /// back to the legacy [`Self::test_path`] probing, which is relative to the
    /// current working directory and therefore correct only when pytest is
    /// invoked from its rootdir.
    fn resolve_test_file(&self, test_file: Option<PathBuf>) -> Result<PathBuf, PytestInfoError> {
        match test_file {
            Some(path) => Ok(path),
            None => self.test_path(),
        }
    }
}

impl FromStr for PytestInfo {
    type Err = PytestInfoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (test_path, test_name_and_stage) = s
            .split_once("::")
            .ok_or(PytestInfoError::CouldNotSplit(s.to_string()))?;

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

impl SnapshotInfo {
    /// Build a [`SnapshotInfo`] from the active pytest test.
    ///
    /// `test_file`, when provided, is the absolute path to the test's source
    /// file (supplied by the pytest plugin from `item.path`). The snapshot
    /// folder is resolved from that file's directory, keeping snapshots next to
    /// the test regardless of the process working directory. The snapshot name
    /// and the stored `Test File Path` description remain derived from the
    /// rootdir-relative `PYTEST_CURRENT_TEST` node id, so committed snapshots
    /// are unaffected.
    pub(crate) fn from_pytest_info(
        info: PytestInfo,
        test_file: Option<PathBuf>,
    ) -> Result<Self, PyErr> {
        let test_file_dir = info
            .resolve_test_file(test_file)?
            .canonicalize()?
            .parent()
            .ok_or_else(|| {
                PyValueError::new_err(format!(
                    "Invalid test_path: {:?}, not yielding a parent directory",
                    info.test_path_raw()
                ))
            })?
            .join("snapshots");

        let test_name = &info.test_name;
        let test_path = info.test_path_raw();
        let file_name = test_path.file_stem().and_then(|s| s.to_str());

        let name = if let Some(f) = file_name {
            format!("{f}_{test_name}")
        } else {
            test_name.to_string()
        };
        Ok(Self {
            snapshot_folder: test_file_dir,
            snapshot_name: name,
            relative_test_file_path: Some(info.test_path_raw().to_string_lossy().to_string()),
            allow_duplicates: false,
        })
    }

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

    /// Ticks the shared duplicate counter and returns the assigned snapshot
    /// name (the base name on first use, `<base>-N` on subsequent uses).
    pub(crate) fn snapshot_name(&self) -> String {
        let mut c = Self::counters();
        let mut test_idx = c.get(&self.snapshot_name).cloned().unwrap_or(0);
        if !self.allow_duplicates {
            test_idx += 1;
            c.insert(self.snapshot_name.clone(), test_idx);
        }

        self.snapshot_name_with_idx(test_idx)
    }

    /// Returns a copy of this `SnapshotInfo` with `suffix` appended to the
    /// snapshot name (e.g. `<test>_<func_name>` for scoping a mock's request
    /// and response snapshots). Not exposed to Python: only used internally to
    /// compose the mock layer's `prepare_mock_call`.
    pub(crate) fn with_name_suffix(&self, suffix: &str) -> Self {
        Self {
            snapshot_name: format!("{}_{}", self.snapshot_name, suffix),
            ..self.clone()
        }
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
            SnapshotContents::Binary(Some(items)) => items.deref().to_owned(),
            SnapshotContents::Binary(None) => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Binary snapshot metadata exists but its data file is missing",
                ))
            }
        })
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_into_pyinfo_happy_path() {
        let s = "tests/a/b/test_thing.py::test_a (call)";
        let pti: Result<PytestInfo, PytestInfoError> = s.parse();
        insta::assert_debug_snapshot!(pti)
    }

    #[test]
    fn test_into_pyinfo_no_trailer() {
        let s = "tests/a/b/test_thing.py::test_a";
        let pti: Result<PytestInfo, PytestInfoError> = s.parse();
        insta::assert_debug_snapshot!(pti)
    }

    #[test]
    fn test_into_pyinfo_failure_case() {
        let s = "tests/a/b/test_thing.py";
        let pti: Result<PytestInfo, PytestInfoError> = s.parse();
        insta::assert_debug_snapshot!(pti)
    }

    #[test]
    fn test_snapshot_info_overrides_from_pytest() {
        let snapshot_info = SnapshotInfo::from_pytest(
            Some("folder_path_override".into()),
            Some("snapshot_name_override".into()),
            false,
            None,
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

    /// The snapshot folder must be derived from the *test file's* directory, not
    /// from the process working directory. When the pytest plugin supplies the
    /// test file's absolute path, resolution must ignore `current_dir()` and the
    /// rootdir-relative node id path entirely.
    #[test]
    fn test_resolve_uses_absolute_test_file_not_cwd() {
        let tmp = std::env::temp_dir().join(format!(
            "pysnaptest_resolve_{}_{}",
            std::process::id(),
            line!()
        ));
        let test_dir = tmp.join("a").join("b");
        std::fs::create_dir_all(&test_dir).unwrap();
        let test_file = test_dir.join("test_thing.py");
        std::fs::write(&test_file, b"# test").unwrap();

        // A rootdir-relative node id that does NOT exist relative to cwd.
        let info: PytestInfo = "pkg/nested/test_thing.py::test_a".parse().unwrap();
        let snapshot_info =
            SnapshotInfo::from_pytest_info(info, Some(test_file.clone())).unwrap();

        // Folder is next to the real test file, mirroring insta.
        assert_eq!(
            snapshot_info.snapshot_folder(),
            &test_dir.canonicalize().unwrap().join("snapshots")
        );
        // Name is still derived from the node id's file stem + test name.
        assert_eq!(snapshot_info.snapshot_name, "test_thing_test_a");
        // The stored description keeps the rootdir-relative node id path.
        assert_eq!(
            snapshot_info.relative_test_file_path.as_deref(),
            Some("pkg/nested/test_thing.py")
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// A bogus rootdir-relative path that exists neither relative to cwd nor as
    /// a `./<filename>` fallback must surface an error rather than silently
    /// resolving to the wrong directory (the pre-fix crash path), when no
    /// absolute test file is provided.
    #[test]
    fn test_resolve_without_test_file_errors_on_missing_path() {
        let info: PytestInfo =
            "does/not/exist/anywhere/test_missing.py::test_a".parse().unwrap();
        let result = SnapshotInfo::from_pytest_info(info, None);
        assert!(
            result.is_err(),
            "expected an error when the node-id path cannot be resolved from cwd"
        );
    }
}
