use std::{path::Path, str::FromStr};

use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

use crate::errors::AppResult;

pub async fn connect(path: &Path) -> AppResult<SqlitePool> {
    let options = SqliteConnectOptions::from_str(&path.to_string_lossy())?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePool::connect_with(options).await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "database migration failed");
            crate::errors::AppError::Database(error.to_string())
        })?;
    Ok(pool)
}

pub async fn recover_interrupted_downloads(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query(
        "UPDATE downloads
         SET status = 'failed', error_code = 'interrupted',
             error_message = 'The application closed before this download finished',
             completed_at = CURRENT_TIMESTAMP
         WHERE status IN ('analyzing', 'downloading', 'processing', 'verifying', 'finalizing')",
    )
    .execute(pool)
    .await?;
    Ok(())
}
