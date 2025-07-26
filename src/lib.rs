#![deny(clippy::unwrap_used)]

use pyo3::{pymodule,
    types::{PyModule, PyModuleMethods},
    wrap_pyfunction, Bound, PyResult,
};

mod mocks;
mod common;

pub use mocks::*;
pub use common::*;

use std::{collections::HashMap, path::PathBuf};

use csv::ReaderBuilder;
use pyo3::prelude::*;

#[pyfunction]
#[pyo3(signature = (test_info, result, redactions=None))]
pub fn assert_json_snapshot(
    test_info: &SnapshotInfo,
    result: &Bound<'_, PyAny>,
    redactions: Option<HashMap<String, RedactionType>>,
) -> PyResult<()> {
    let res: serde_json::Value = pythonize::depythonize(result)?;
    let snapshot_name = test_info.snapshot_name();
    let mut settings: insta::Settings = test_info.try_into()?;

    for (selector, redaction) in redactions.unwrap_or_default() {
        settings.add_redaction(selector.as_str(), redaction);
    }

    settings.bind(|| {
        insta::assert_json_snapshot!(snapshot_name, res);
    });
    Ok(())
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
        settings.add_redaction(selector.as_str(), redaction);
    }

    settings.bind(|| {
        insta::assert_csv_snapshot!(snapshot_name, res);
    });
    Ok(())
}

#[pyfunction]
pub fn assert_binary_snapshot(
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
pub fn assert_snapshot(test_info: &SnapshotInfo, result: &Bound<'_, PyAny>) -> PyResult<()> {
    let snapshot_name = test_info.snapshot_name();
    let settings: insta::Settings = test_info.try_into()?;
    settings.bind(|| {
        insta::assert_snapshot!(snapshot_name, result);
    });
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