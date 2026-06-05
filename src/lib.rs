#![deny(clippy::unwrap_used)]

use pyo3::{
    pymodule,
    types::{PyModule, PyModuleMethods},
    wrap_pyfunction, Bound, PyResult,
};

mod common;
mod compression;
mod errors;
mod mocks;

pub use common::*;
pub use errors::*;
pub use mocks::*;

use std::{collections::HashMap, path::PathBuf};

use csv::ReaderBuilder;
use pyo3::exceptions::PyValueError;
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
    // Keep every field as a string so the csv crate cannot infer types and
    // silently rewrite values on round-trip (e.g. 007 -> 7, 1.10 -> 1.1,
    // 1e3 -> 1000.0). Column redactions still apply since the headers are
    // preserved as the first row.
    let mut rdr = ReaderBuilder::new().from_reader(result.as_bytes());
    let header_row: Vec<serde_json::Value> = rdr
        .headers()
        .map_err(|e| PyValueError::new_err(format!("Failed to parse csv headers: {e}")))?
        .iter()
        .map(|h| serde_json::Value::String(h.to_string()))
        .collect();

    let mut res: Vec<Vec<serde_json::Value>> = vec![header_row];
    for record in rdr.records() {
        let record = record
            .map_err(|e| PyValueError::new_err(format!("Failed to parse csv record: {e}")))?;
        res.push(
            record
                .iter()
                .map(|field| serde_json::Value::String(field.to_string()))
                .collect(),
        );
    }

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
#[pyo3(signature = (test_info, result, algorithm="gzip"))]
pub fn assert_compressed_snapshot(
    test_info: &SnapshotInfo,
    result: Vec<u8>,
    algorithm: &str,
) -> PyResult<()> {
    let algorithm = compression::CompressionAlgorithm::parse(algorithm)?;

    // Validate the input actually decompresses before storing it, so callers get
    // a clear error instead of an unreadable binary snapshot.
    algorithm.decompress(&result).map_err(|e| {
        PyValueError::new_err(format!(
            "Failed to decompress the provided bytes as {algorithm:?}: {e}"
        ))
    })?;

    let snapshot_name = test_info.snapshot_name();
    let extension = algorithm.extension();
    let mut settings: insta::Settings = test_info.try_into()?;
    // Store the compressed bytes on disk but compare on the decompressed
    // contents (see `compression::CompressionComparator`).
    settings.set_comparator(Box::new(compression::CompressionComparator::new(algorithm)));
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
    m.add_function(wrap_pyfunction!(assert_compressed_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(mock_json_snapshot, m)?)?;
    m.add_class::<PySnapshot>()?;
    Ok(())
}
