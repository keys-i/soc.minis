use std::{io, path::Path};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SocError {
    #[error("{0}")]
    Message(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("Cannot use the saved SOC state: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("The remote service could not complete the request: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Cannot build the Word document: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Cannot read the room catalogue: {0}")]
    Csv(#[from] csv::Error),
}

pub type Result<T> = std::result::Result<T, SocError>;

pub trait IoContext<T> {
    fn at(self, action: impl Into<String>) -> Result<T>;
    fn at_path(self, action: &str, path: &Path) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, action: impl Into<String>) -> Result<T> {
        self.map_err(|source| SocError::Io {
            context: action.into(),
            source,
        })
    }

    fn at_path(self, action: &str, path: &Path) -> Result<T> {
        self.at(format!("{action} {}", path.display()))
    }
}

pub fn invalid(message: impl Into<String>) -> SocError {
    SocError::Message(message.into())
}
