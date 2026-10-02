use sqlx::SqlitePool;

use crate::{
    domain::{DownloadJob, DownloadStatus},
    errors::{AppError, AppResult},
};

pub struct DownloadRepository {
    pool: SqlitePool,
}

impl DownloadRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        id: &str,
        title: &str,
        source_url: &str,
        folder_id: Option<&str>,
    ) -> AppResult<DownloadJob> {
        sqlx::query(
            "INSERT INTO downloads (id, title, source_url, status, folder_id)
             VALUES (?, ?, ?, 'queued', (SELECT id FROM folders WHERE id = ?))",
        )
        .bind(id)
        .bind(title)
        .bind(source_url)
        .bind(folder_id)
        .execute(&self.pool)
        .await?;
        self.get(id).await
    }

    pub async fn get(&self, id: &str) -> AppResult<DownloadJob> {
        Ok(
            sqlx::query_as::<_, DownloadJob>("SELECT * FROM downloads WHERE id = ?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn list(&self) -> AppResult<Vec<DownloadJob>> {
        Ok(sqlx::query_as::<_, DownloadJob>(
            "SELECT * FROM downloads ORDER BY created_at DESC LIMIT 200",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn update_progress(
        &self,
        id: &str,
        progress: f64,
        downloaded_bytes: i64,
        total_bytes: Option<i64>,
        bytes_per_second: Option<i64>,
    ) -> AppResult<Option<DownloadJob>> {
        let result = sqlx::query(
            "UPDATE downloads
             SET progress = ?, downloaded_bytes = ?, total_bytes = ?, bytes_per_second = ?
             WHERE id = ? AND status = 'downloading'",
        )
        .bind(progress)
        .bind(downloaded_bytes)
        .bind(total_bytes)
        .bind(bytes_per_second)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get(id).await.map(Some)
    }

    pub async fn transition(&self, id: &str, next: DownloadStatus) -> AppResult<DownloadJob> {
        let current = self.get(id).await?;
        current.status.transition(next)?;
        let terminal = matches!(
            next,
            DownloadStatus::Completed | DownloadStatus::Failed | DownloadStatus::Cancelled
        );
        let result = sqlx::query(
            "UPDATE downloads SET status = ?,
             started_at = CASE WHEN ? = 'downloading' AND started_at IS NULL THEN CURRENT_TIMESTAMP ELSE started_at END,
             completed_at = CASE WHEN ? THEN CURRENT_TIMESTAMP ELSE completed_at END,
             progress = CASE WHEN ? = 'completed' THEN 1 ELSE progress END
             WHERE id = ? AND status = ?",
        )
        .bind(next.as_str()).bind(next.as_str()).bind(terminal).bind(next.as_str())
        .bind(id).bind(current.status.as_str()).execute(&self.pool).await?;
        if result.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "download state changed concurrently".into(),
            ));
        }
        self.get(id).await
    }

    pub async fn fail(&self, id: &str, error: &AppError) -> AppResult<DownloadJob> {
        let status = if matches!(error, AppError::Cancelled) {
            DownloadStatus::Cancelled
        } else {
            DownloadStatus::Failed
        };
        let current = self.get(id).await?;
        if !current.status.can_transition_to(status) {
            return Ok(current);
        }
        sqlx::query("UPDATE downloads SET status = ?, error_code = ?, error_message = ?, completed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(status.as_str()).bind(if status == DownloadStatus::Cancelled { "cancelled" } else { "download_failed" })
            .bind(error.public_message()).bind(id).execute(&self.pool).await?;
        self.get(id).await
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
