use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("malformed .geo file")]
    Malformed,
    #[error("unsupported .geo file")]
    Unsupported,
    #[error("early EOF")]
    EarlyEof,
    #[error("I/O error")]
    Io(#[from] std::io::Error),
}
