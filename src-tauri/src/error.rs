use serde::{Serialize, Serializer};

/// Application-wide error type. Serialized to the frontend as `{ kind, message }`.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    fn kind(&self) -> &'static str {
        match self {
            AppError::Io(_) => "io",
            AppError::Validation(_) => "validation",
            AppError::NotFound(_) => "not_found",
            AppError::Other(_) => "other",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
