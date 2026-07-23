#![deny(clippy::unwrap_used)]

use pyo3::{
    pymodule,
    types::{PyModule, PyModuleMethods},
    wrap_pyfunction, Bound, PyResult,
};

mod common;
mod errors;
mod mocks;
mod panic;

pub use common::*;
pub use errors::*;
pub use mocks::*;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use csv::ReaderBuilder;
use insta::output::SnapshotPrinter;
use insta::Snapshot;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Binds insta settings (path, redactions) and asserts a JSON snapshot under an
/// explicit `snapshot_name`.
///
/// The `insta::assert_json_snapshot!` call is emitted at the macro's *invocation*
/// site, so the on-disk `<module_path>__<name>@pysnap.snap` prefix follows the
/// module that uses this macro. Keeping the assertion in one place lets both the
/// counter-based [`assert_json_snapshot`] and the mock layer's
/// `assert_json_snapshot_named` share identical settings and panic handling.
#[macro_export]
macro_rules! bind_json_snapshot {
    ($test_info:expr, $res:expr, $snapshot_name:expr, $redactions:expr) => {{
        let mut settings: insta::Settings = $test_info.try_into()?;
        for (selector, redaction) in $redactions.unwrap_or_default() {
            settings.add_redaction(selector.as_str(), redaction);
        }
        let snapshot_name = $snapshot_name;
        let snapshot_label = snapshot_name.clone();
        $crate::panic::run_snapshot_assertion(&snapshot_label, || {
            settings.bind(|| {
                insta::assert_json_snapshot!(snapshot_name, $res);
            });
        })
    }};
}

#[pyfunction]
#[pyo3(signature = (test_info, result, redactions=None))]
pub fn assert_json_snapshot(
    test_info: &SnapshotInfo,
    result: &Bound<'_, PyAny>,
    redactions: Option<HashMap<String, RedactionType>>,
) -> PyResult<()> {
    let res: serde_json::Value = pythonize::depythonize(result)?;
    let snapshot_name = test_info.snapshot_name();
    bind_json_snapshot!(test_info, res, snapshot_name, redactions)
}

#[pyfunction]
#[pyo3(signature = (test_info, result, redactions=None))]
pub fn assert_csv_snapshot(
    test_info: &SnapshotInfo,
    result: &str,
    redactions: Option<HashMap<String, RedactionType>>,
) -> PyResult<()> {
    let mut rdr = ReaderBuilder::new().from_reader(result.as_bytes());
    let columns: Vec<Vec<serde_json::Value>> = vec![rdr
        .headers()
        .map_err(|e| PyValueError::new_err(format!("Failed to read CSV headers: {e}")))?
        .into_iter()
        .map(|h| h.into())
        .collect()];
    let records = rdr
        .into_deserialize()
        .collect::<Result<Vec<Vec<serde_json::Value>>, _>>()
        .map_err(|e| PyValueError::new_err(format!("Failed to parse CSV records: {e}")))?;
    let res: Vec<Vec<serde_json::Value>> = columns.into_iter().chain(records).collect();

    let snapshot_name = test_info.snapshot_name();
    let mut settings: insta::Settings = test_info.try_into()?;

    for (selector, redaction) in redactions.unwrap_or_default() {
        settings.add_redaction(selector.as_str(), redaction);
    }

    let snapshot_label = snapshot_name.clone();
    panic::run_snapshot_assertion(&snapshot_label, || {
        settings.bind(|| {
            insta::assert_csv_snapshot!(snapshot_name, res);
        });
    })
}

#[pyfunction]
pub fn assert_binary_snapshot(
    test_info: &SnapshotInfo,
    extension: &str,
    result: Vec<u8>,
) -> PyResult<()> {
    let snapshot_name = test_info.snapshot_name();
    let settings: insta::Settings = test_info.try_into()?;
    let snapshot_label = snapshot_name.clone();
    panic::run_snapshot_assertion(&snapshot_label, || {
        settings.bind(|| {
            insta::assert_binary_snapshot!(format!("{snapshot_name}.{extension}").as_str(), result);
        });
    })
}

#[pyfunction]
pub fn assert_snapshot(test_info: &SnapshotInfo, result: &Bound<'_, PyAny>) -> PyResult<()> {
    let snapshot_name = test_info.snapshot_name();
    let settings: insta::Settings = test_info.try_into()?;
    let snapshot_label = snapshot_name.clone();
    panic::run_snapshot_assertion(&snapshot_label, || {
        settings.bind(|| {
            insta::assert_snapshot!(snapshot_name, result);
        });
    })
}

/// Removes a snapshot's binary sidecar data file, if it has one.
///
/// The sidecar path is resolved through insta's own [`Snapshot::build_binary_path`]
/// so we only ever touch the data file insta actually wrote (e.g. `@pysnap.snap.parquet`)
/// and never an unrelated sibling such as a `.snap.new` pending file. Returns the
/// removed sidecar path, or `None` when the snapshot is text-only or has no sidecar.
fn remove_binary_sidecar(path: &Path, snapshot: &Snapshot) -> PyResult<Option<PathBuf>> {
    if let Some(sidecar) = snapshot.build_binary_path(path) {
        if sidecar.exists() {
            std::fs::remove_file(&sidecar).map_err(|e| {
                PyValueError::new_err(format!("Unable to remove binary sidecar {sidecar:?}: {e}"))
            })?;
            return Ok(Some(sidecar));
        }
    }
    Ok(None)
}

/// Removes a pending `.snap.new` file and, for binary snapshots, its sidecar data file.
fn remove_pending_files(pending_path: &Path, snapshot: &Snapshot) -> PyResult<()> {
    remove_binary_sidecar(pending_path, snapshot)?;
    std::fs::remove_file(pending_path).map_err(|e| {
        PyValueError::new_err(format!(
            "Unable to remove pending snapshot {pending_path:?}: {e}"
        ))
    })
}

/// Ensures a path is a pending snapshot file (has a trailing `.new` extension).
fn ensure_pending(pending_path: &Path) -> PyResult<()> {
    if pending_path.extension().and_then(|e| e.to_str()) == Some("new") {
        Ok(())
    } else {
        Err(PyValueError::new_err(format!(
            "Not a pending snapshot file (expected a trailing '.new'): {pending_path:?}"
        )))
    }
}

/// Accepts a pending snapshot by persisting it to its target `.snap` file.
///
/// The pending snapshot is loaded through insta so the committed snapshot is written with the
/// correct format (pending-only metadata is trimmed and binary sidecars are handled). The pending
/// `.snap.new` file (and any binary sidecar) is removed afterwards. Returns the target path.
#[pyfunction]
pub fn accept_pending_snapshot(pending_path: PathBuf) -> PyResult<PathBuf> {
    ensure_pending(&pending_path)?;
    let target = pending_path.with_extension("");
    let snapshot = Snapshot::from_file(&pending_path).map_err(|e| {
        PyValueError::new_err(format!(
            "Unable to load pending snapshot from {pending_path:?}, details: {e}"
        ))
    })?;
    snapshot.save(&target).map_err(|e| {
        PyValueError::new_err(format!(
            "Unable to save snapshot to {target:?}, details: {e}"
        ))
    })?;
    remove_pending_files(&pending_path, &snapshot)?;
    Ok(target)
}

/// Rejects a pending snapshot by deleting its `.snap.new` file (and any binary sidecar).
#[pyfunction]
pub fn reject_pending_snapshot(pending_path: PathBuf) -> PyResult<()> {
    ensure_pending(&pending_path)?;
    match Snapshot::from_file(&pending_path) {
        Ok(snapshot) => remove_pending_files(&pending_path, &snapshot),
        // A corrupt/unreadable pending file still needs to be cleared.
        Err(_) => std::fs::remove_file(&pending_path).map_err(|e| {
            PyValueError::new_err(format!(
                "Unable to remove pending snapshot {pending_path:?}: {e}"
            ))
        }),
    }
}

/// Deletes a committed snapshot file and its binary sidecar data file (if any).
///
/// The sidecar is resolved through insta's own [`Snapshot::build_binary_path`],
/// the same primitive used to clean up pending files, so obsolete-snapshot
/// deletion (`pysnaptest unused --delete`) removes exactly what insta wrote and
/// never an unrelated sibling such as a `.snap.new` pending file. Returns the
/// removed paths (the sidecar first, when present, then the metadata file). A
/// corrupt/unreadable metadata file is still removed.
#[pyfunction]
pub fn delete_snapshot(snapshot_path: PathBuf) -> PyResult<Vec<PathBuf>> {
    let mut removed = Vec::new();
    if let Ok(snapshot) = Snapshot::from_file(&snapshot_path) {
        if let Some(sidecar) = remove_binary_sidecar(&snapshot_path, &snapshot)? {
            removed.push(sidecar);
        }
    }
    std::fs::remove_file(&snapshot_path).map_err(|e| {
        PyValueError::new_err(format!("Unable to remove snapshot {snapshot_path:?}: {e}"))
    })?;
    removed.push(snapshot_path);
    Ok(removed)
}

/// Prints insta's own diff for a pending snapshot against its committed target.
///
/// This renders the exact colored diff insta shows during a failing assertion, so the
/// review workflow leans on insta rather than re-implementing diffing.
#[pyfunction]
#[pyo3(signature = (pending_path, workspace_root=None))]
pub fn print_pending_diff(pending_path: PathBuf, workspace_root: Option<PathBuf>) -> PyResult<()> {
    ensure_pending(&pending_path)?;
    let new_snapshot = Snapshot::from_file(&pending_path).map_err(|e| {
        PyValueError::new_err(format!(
            "Unable to load pending snapshot from {pending_path:?}, details: {e}"
        ))
    })?;
    let target = pending_path.with_extension("");
    let old_snapshot = if target.exists() {
        Some(Snapshot::from_file(&target).map_err(|e| {
            PyValueError::new_err(format!(
                "Unable to load snapshot from {target:?}, details: {e}"
            ))
        })?)
    } else {
        None
    };
    let root = workspace_root
        .or_else(|| std::env::var_os("INSTA_WORKSPACE_ROOT").map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let title = new_snapshot
        .snapshot_name()
        .unwrap_or("snapshot")
        .to_string();
    let mut printer = SnapshotPrinter::new(&root, old_snapshot.as_ref(), &new_snapshot);
    printer.set_show_diff(true);
    printer.set_show_info(true);
    printer.set_title(Some(&title));
    printer.set_snapshot_file(Some(&target));
    printer.print();
    Ok(())
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
            "{module_path}__{}{SNAPSHOT_FILE_SUFFIX}",
            self.last_snapshot_name()
        )))
    }

    pub fn next_snapshot_path(&self, module_path: Option<String>) -> PyResult<PathBuf> {
        let module_path = module_path
            .unwrap_or(module_path!().to_string())
            .replace("::", "__");
        Ok(self.snapshot_folder.join(format!(
            "{module_path}__{}{SNAPSHOT_FILE_SUFFIX}",
            self.next_snapshot_name()
        )))
    }
}

#[pymodule]
#[pyo3(name = "_pysnaptest")]
fn pysnaptest(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SnapshotInfo>()?;

    m.add("SNAPSHOT_SUFFIX", SNAPSHOT_FILE_SUFFIX)?;
    m.add_function(wrap_pyfunction!(assert_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(assert_binary_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(assert_json_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(assert_csv_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(prepare_mock_call, m)?)?;
    m.add_function(wrap_pyfunction!(assert_json_snapshot_named, m)?)?;
    m.add_function(wrap_pyfunction!(read_json_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(accept_pending_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(reject_pending_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(delete_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(print_pending_diff, m)?)?;
    m.add_class::<PySnapshot>()?;
    Ok(())
}
