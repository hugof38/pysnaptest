use std::env::VarError;
use std::fmt::{self, Display, Formatter};

use pyo3::PyErr;
use pyo3::exceptions::PyValueError;

#[derive(Debug)]
pub enum PytestInfoError {
    CouldNotSplit(String),
    InvalidEnvVar(VarError),
    NoTestFile,
}

impl From<PytestInfoError> for PyErr {
    fn from(value: PytestInfoError) -> Self {
        match value {
            PytestInfoError::CouldNotSplit(s) => PyValueError::new_err(format!(
                "Expected '::' to be in PYTEST_CURRENT_TEST string ({s})"
            )),
            PytestInfoError::InvalidEnvVar(ve) => match ve {
                VarError::NotPresent => PyValueError::new_err("PYTEST_CURRENT_TEST is not set"),
                VarError::NotUnicode(os_string) =>
                    PyValueError::new_err(format!("PYTEST_CURRENT_TEST is not a valid unicode string: {os_string:#?}")),
            },
            PytestInfoError::NoTestFile => PyValueError::new_err("No test file found"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnapError(pub String);

impl Display for SnapError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SnapError {}

impl From<String> for SnapError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SnapError {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<std::io::Error> for SnapError {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<serde_json::Error> for SnapError {
    fn from(value: serde_json::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<PyErr> for SnapError {
    fn from(value: PyErr) -> Self {
        Self(value.to_string())
    }
}

pub type SnapResult<T> = Result<T, SnapError>;
