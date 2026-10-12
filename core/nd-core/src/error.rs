#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum NdError {
    #[error("storage: {msg}")]
    Storage { msg: String },
    #[error("invalid input: {msg}")]
    Invalid { msg: String },
    #[error("internal: {msg}")]
    Internal { msg: String },
}

impl From<rusqlite::Error> for NdError {
    fn from(e: rusqlite::Error) -> Self {
        NdError::Storage { msg: e.to_string() }
    }
}

impl From<serde_json::Error> for NdError {
    fn from(e: serde_json::Error) -> Self {
        NdError::Invalid { msg: e.to_string() }
    }
}

pub type Result<T> = std::result::Result<T, NdError>;
