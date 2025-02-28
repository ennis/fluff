use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum  Error {
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),
    #[error("unexpected property type")]
    UnexpectedPropertyType,
    #[error("property not found")]
    PropertyNotFound,
    #[error("unexpected data type")]
    UnexpectedDataType,
}

/// Creates an `Error` with an `io::ErrorKind::InvalidData` error.
pub(crate) fn invalid_data(reason: &str) -> Error {
    Error::IO(io::Error::new(io::ErrorKind::InvalidData, reason))
}