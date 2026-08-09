use sqlx::SqlitePool;

use crate::{
    domain::Media,
    errors::{AppError, AppResult},
};

pub struct MediaRepository {
    pool: SqlitePool,
}

impl MediaRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self, folder_id: Option<&str>) -> AppResult<Vec<Media>> {
        let items = match folder_id {
            Some(folder_id) => {
                sqlx::query_as::<_, Media>(
                    "SELECT * FROM media WHERE folder_id = ? ORDER BY created_at DESC",
                )
                .bind(folder_id)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, Media>("SELECT * FROM media ORDER BY created_at DESC")
                    .fetch_all(&self.pool)
                    .await?
            }
        };
        Ok(items)
    }

    pub async fn search(&self, query: &str) -> AppResult<Vec<Media>> {
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        Ok(sqlx::query_as::<_, Media>(
            "SELECT * FROM media
             WHERE title LIKE ? ESCAPE '\\'
                OR coalesce(creator, '') LIKE ? ESCAPE '\\'
                OR source_platform LIKE ? ESCAPE '\\'
             ORDER BY created_at DESC LIMIT 200",
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn get(&self, id: &str) -> AppResult<Media> {
        Ok(
            sqlx::query_as::<_, Media>("SELECT * FROM media WHERE id = ?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn rename(&self, id: &str, title: &str) -> AppResult<Media> {
        let result =
            sqlx::query("UPDATE media SET title = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(title)
                .bind(id)
                .execute(&self.pool)
                .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        self.get(id).await
    }

    pub async fn move_to(&self, id: &str, folder_id: Option<&str>) -> AppResult<Media> {
        let result = sqlx::query(
            "UPDATE media SET folder_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(folder_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        self.get(id).await
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }
}
