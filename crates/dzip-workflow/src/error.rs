use std::fmt;

#[cfg(feature = "protocol")]
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, WorkflowError>;

#[derive(Debug)]
pub enum WorkflowError {
    Dzip(dzip::DzipError),
    InvalidInput(String),
    SessionNotFound(u64),
    EntryNotFound(usize),
    Io(std::io::Error),
}

impl WorkflowError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dzip(error) => error.fmt(formatter),
            Self::InvalidInput(message) => formatter.write_str(message),
            Self::SessionNotFound(id) => write!(formatter, "archive session {id} does not exist"),
            Self::EntryNotFound(id) => write!(formatter, "archive entry {id} does not exist"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for WorkflowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Dzip(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::InvalidInput(_) | Self::SessionNotFound(_) | Self::EntryNotFound(_) => None,
        }
    }
}

impl From<dzip::DzipError> for WorkflowError {
    fn from(error: dzip::DzipError) -> Self {
        Self::Dzip(error)
    }
}

impl From<std::io::Error> for WorkflowError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(feature = "protocol")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowErrorCode {
    Archive,
    InvalidInput,
    SessionNotFound,
    EntryNotFound,
    Io,
}

#[cfg(feature = "protocol")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFailure {
    pub code: WorkflowErrorCode,
    pub message: String,
}

#[cfg(feature = "protocol")]
impl From<WorkflowError> for WorkflowFailure {
    fn from(error: WorkflowError) -> Self {
        let code = match &error {
            WorkflowError::Dzip(_) => WorkflowErrorCode::Archive,
            WorkflowError::InvalidInput(_) => WorkflowErrorCode::InvalidInput,
            WorkflowError::SessionNotFound(_) => WorkflowErrorCode::SessionNotFound,
            WorkflowError::EntryNotFound(_) => WorkflowErrorCode::EntryNotFound,
            WorkflowError::Io(_) => WorkflowErrorCode::Io,
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

#[cfg(feature = "protocol")]
impl fmt::Display for WorkflowFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
