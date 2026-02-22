use thiserror::Error;

/// Application-level errors for ferritex.
#[derive(Debug, Error)]
pub enum FerritexError {
    /// Wrapper around I/O failures.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Returned from stubs that are not yet implemented.
    #[error("Not implemented: {0}")]
    NotImplemented(&'static str),
}
