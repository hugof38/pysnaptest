use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;

use csv::ReaderBuilder;
use insta::internals::SnapshotContents;
use insta::Snapshot;
use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict, PyTuple};

use crate::{RedactionType, SnapshotInfo};

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
