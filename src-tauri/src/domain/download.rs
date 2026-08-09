use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Type};

use crate::errors::{AppError, AppResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Analyzing,
    Downloading,
    Processing,
    Verifying,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Analyzing => "analyzing",
            Self::Downloading => "downloading",
            Self::Processing => "processing",
            Self::Verifying => "verifying",
            Self::Finalizing => "finalizing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use DownloadStatus as S;
        matches!(
            (self, next),
            (S::Queued, S::Analyzing)
                | (S::Analyzing, S::Downloading)
                | (S::Downloading, S::Processing)
                | (S::Downloading, S::Verifying)
                | (S::Processing, S::Verifying)
                | (S::Verifying, S::Finalizing)
                | (S::Finalizing, S::Completed)
                | (
                    S::Queued
                        | S::Analyzing
                        | S::Downloading
                        | S::Processing
                        | S::Verifying
                        | S::Finalizing,
                    S::Failed | S::Cancelled
                )
        )
    }

    pub fn transition(self, next: Self) -> AppResult<Self> {
        self.can_transition_to(next).then_some(next).ok_or_else(|| {
            AppError::Conflict(format!("invalid download transition: {self:?} -> {next:?}"))
        })
    }
}

#[derive(Clone, Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub id: String,
    pub media_id: Option<String>,
    pub title: String,
    pub source_url: String,
    pub status: DownloadStatus,
    pub progress: f64,
    pub downloaded_bytes: Option<i64>,
    pub total_bytes: Option<i64>,
    pub bytes_per_second: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDownloadInput {
    pub analysis_id: String,
    pub option_id: String,
    pub title: String,
    pub folder_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::DownloadStatus as S;

    #[test]
    fn happy_path_transitions_are_valid() {
        let states = [
            S::Queued,
            S::Analyzing,
            S::Downloading,
            S::Processing,
            S::Verifying,
            S::Finalizing,
            S::Completed,
        ];
        for pair in states.windows(2) {
            assert!(pair[0].can_transition_to(pair[1]));
        }
    }

    #[test]
    fn terminal_states_cannot_transition() {
        assert!(!S::Completed.can_transition_to(S::Queued));
        assert!(!S::Failed.can_transition_to(S::Queued));
        assert!(!S::Cancelled.can_transition_to(S::Queued));
    }
}
