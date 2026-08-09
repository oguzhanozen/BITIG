use sqlx::SqlitePool;

use crate::{
    domain::Folder,
    errors::{AppError, AppResult},
};

pub struct FolderRepository {
    pool: SqlitePool,
}

impl FolderRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> AppResult<Vec<Folder>> {
        Ok(sqlx::query_as::<_, Folder>(
            "SELECT id, parent_id, name, created_at, updated_at
             FROM folders ORDER BY lower(name), created_at",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn exists(&self, id: &str) -> AppResult<bool> {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM folders WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        Ok(count == 1)
    }

    pub async fn create(&self, id: &str, parent_id: Option<&str>, name: &str) -> AppResult<Folder> {
        let result = sqlx::query("INSERT INTO folders (id, parent_id, name) VALUES (?, ?, ?)")
            .bind(id)
            .bind(parent_id)
            .bind(name)
            .execute(&self.pool)
            .await;
        if let Err(error) = result {
            if error
                .as_database_error()
                .is_some_and(|detail| detail.is_unique_violation())
            {
                return Err(AppError::Conflict(
                    "a folder with this name already exists here".into(),
                ));
            }
            return Err(error.into());
        }
        self.get(id).await
    }

    pub async fn get(&self, id: &str) -> AppResult<Folder> {
        Ok(sqlx::query_as::<_, Folder>(
            "SELECT id, parent_id, name, created_at, updated_at FROM folders WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn rename(&self, id: &str, name: &str) -> AppResult<Folder> {
        let result =
            sqlx::query("UPDATE folders SET name = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(name)
                .bind(id)
                .execute(&self.pool)
                .await;
        let result = match result {
            Ok(result) => result,
            Err(error)
                if error
                    .as_database_error()
                    .is_some_and(|detail| detail.is_unique_violation()) =>
            {
                return Err(AppError::Conflict(
                    "a folder with this name already exists here".into(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        self.get(id).await
    }

    pub async fn is_empty(&self, id: &str) -> AppResult<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM folders WHERE parent_id = ?)
                  + (SELECT count(*) FROM media WHERE folder_id = ?)",
        )
        .bind(id)
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count == 0)
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM folders WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }
}
