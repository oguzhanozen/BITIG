use serde::{Serialize, Serializer};
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("The supplied value is invalid: {0}")]
    Validation(String),
    #[error("Only HTTP and HTTPS media URLs are supported")]
    InvalidUrl,
    #[error("The media analysis tool is not available")]
    ToolUnavailable,
    #[error("A bundled media tool failed its integrity check: {0}")]
    ToolIntegrity(String),
    #[error("The media URL could not be analyzed: {0}")]
    Analysis(String),
    #[error("The media download failed: {0}")]
    Download(String),
    #[error("The downloaded media could not be validated")]
    MediaValidation,
    #[error("An external tool produced too much output")]
    ProcessOutputTooLarge,
    #[error("An external tool did not finish in time")]
    ProcessTimeout,
    #[error("The operation was cancelled")]
    Cancelled,
    #[error("The requested item was not found")]
    NotFound,
    #[error("The operation conflicts with existing data: {0}")]
    Conflict(String),
    #[error("The operation is blocked because the item is not empty")]
    NotEmpty,
    #[error("The requested path is outside managed storage")]
    UnsafePath,
    #[error("A storage operation failed: {0}")]
    Storage(String),
    #[error("A database operation failed: {0}")]
    Database(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorDto<'a> {
    code: &'a str,
    message: String,
}

impl AppError {
    fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation",
            Self::InvalidUrl => "invalid_url",
            Self::ToolUnavailable => "tool_unavailable",
            Self::ToolIntegrity(_) => "tool_integrity_failed",
            Self::Analysis(_) => "analysis_failed",
            Self::Download(_) => "download_failed",
            Self::MediaValidation => "media_validation_failed",
            Self::ProcessOutputTooLarge => "process_output_too_large",
            Self::ProcessTimeout => "process_timeout",
            Self::Cancelled => "cancelled",
            Self::NotFound => "not_found",
            Self::Conflict(_) => "conflict",
            Self::NotEmpty => "not_empty",
            Self::UnsafePath => "unsafe_path",
            Self::Storage(_) => "storage_error",
            Self::Database(_) => "database_error",
        }
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::Database(_) => "The local library could not be updated.".into(),
            Self::Storage(_) => "The managed media file could not be updated.".into(),
            Self::Analysis(_) => {
                "This URL could not be analyzed. Check that it is supported and try again.".into()
            }
            Self::Download(_) => {
                "The media could not be downloaded. You can retry it from Downloads.".into()
            }
            Self::ToolIntegrity(_) => {
                "A bundled media tool failed its integrity check. Reinstall BITIG.".into()
            }
            _ => self.to_string(),
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ErrorDto {
            code: self.code(),
            message: self.public_message(),
        }
        .serialize(serializer)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        tracing::error!(error = %value, "database operation failed");
        match value {
            sqlx::Error::RowNotFound => Self::NotFound,
            other => Self::Database(other.to_string()),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        tracing::error!(error = %value, "storage operation failed");
        Self::Storage(value.to_string())
    }
}
