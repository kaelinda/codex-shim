use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("which: {0}")]
    Which(#[from] which::Error),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn msg<S: Into<String>>(s: S) -> Self {
        Self::Other(s.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
