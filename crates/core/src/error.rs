use std::io;
use std::path::PathBuf;

/// Errors returned by the core library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the operation was cancelled")]
    Cancelled,

    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("not an NTFS volume")]
    NotNtfs,

    #[error("unreadable NTFS metadata: {0}")]
    CorruptNtfs(String),

    #[error("{0}")]
    InvalidInput(String),

    #[error("snapshot error: {0}")]
    Snapshot(String),
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
