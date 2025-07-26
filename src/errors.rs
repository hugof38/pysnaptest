use std::env::VarError;
use std::fmt::{self, Display, Formatter};

use pyo3::exceptions::PyValueError;
use pyo3::PyErr;
use pythonize::PythonizeError;

#[derive(Debug)]
pub enum PytestInfoError {
    CouldNotSplit(String),
    InvalidEnvVar(VarError),
    NoTestFile,
}

impl Display for PytestInfoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PytestInfoError::CouldNotSplit(s) => {
                write!(f, "Expected '::' to be in PYTEST_CURRENT_TEST string ({s})")
            }
            PytestInfoError::InvalidEnvVar(e) => match e {
                VarError::NotPresent => write!(f, "PYTEST_CURRENT_TEST is not set"),
                VarError::NotUnicode(os_string) => {
                    write!(f, "PYTEST_CURRENT_TEST is not a valid unicode string: {os_string:#?}")
                }
            },
            PytestInfoError::NoTestFile => write!(f, "No test file found"),
        }
    }
}

impl From<PytestInfoError> for PyErr {
    fn from(value: PytestInfoError) -> Self {
        match value {
            PytestInfoError::CouldNotSplit(s) => {
                PyValueError::new_err(format!("Expected '::' to be in PYTEST_CURRENT_TEST string ({s})"))
            }
            PytestInfoError::InvalidEnvVar(ve) => match ve {
                VarError::NotPresent => PyValueError::new_err("PYTEST_CURRENT_TEST is not set"),
                VarError::NotUnicode(os_string) => PyValueError::new_err(format!(
                    "PYTEST_CURRENT_TEST is not a valid unicode string: {os_string:#?}"
                )),
            },
            PytestInfoError::NoTestFile => PyValueError::new_err("No test file found"),
        }
    }
}

#[derive(Debug)]
pub enum SnapError {
    Msg(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Py(PyErr),
    PytestInfo(PytestInfoError),
    Pythonize(PythonizeError),
    Other(Box<dyn std::error::Error>)
}

impl Display for SnapError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            SnapError::Msg(m) => write!(f, "{m}"),
            SnapError::Io(e) => write!(f, "{e}"),
            SnapError::Json(e) => write!(f, "{e}"),
            SnapError::Py(e) => write!(f, "{e}"),
            SnapError::PytestInfo(e) => write!(f, "{e}"),
            SnapError::Pythonize(e) => write!(f, "{e}"),
            SnapError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SnapError {}

impl From<String> for SnapError {
    fn from(value: String) -> Self {
        SnapError::Msg(value)
    }
}

impl From<&str> for SnapError {
    fn from(value: &str) -> Self {
        SnapError::Msg(value.to_string())
    }
}

impl From<std::io::Error> for SnapError {
    fn from(value: std::io::Error) -> Self {
        SnapError::Io(value)
    }
}

impl From<PythonizeError> for SnapError {
    fn from(value: PythonizeError) -> Self {
        SnapError::Pythonize(value)
    }
}

impl From<Box<dyn std::error::Error>> for SnapError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        SnapError::Other(err)
    }
}

impl From<serde_json::Error> for SnapError {
    fn from(value: serde_json::Error) -> Self {
        SnapError::Json(value)
    }
}

impl From<PyErr> for SnapError {
    fn from(value: PyErr) -> Self {
        SnapError::Py(value)
    }
}

impl From<PytestInfoError> for SnapError {
    fn from(value: PytestInfoError) -> Self {
        SnapError::PytestInfo(value)
    }
}

pub type SnapResult<T> = Result<T, SnapError>;
