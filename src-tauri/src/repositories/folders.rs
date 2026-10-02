use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::{
    domain::{Folder, ReorderPlacement},
    errors::{AppError, AppResult},
};

pub struct FolderRepository {
    pool: SqlitePool,
}

pub struct FolderDeletePlan {
    transaction: Transaction<'static, Sqlite>,
    folder_ids: Vec<String>,
    pub media_paths: Vec<(String, Option<String>)>,
}

impl FolderRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> AppResult<Vec<Folder>> {
        Ok(sqlx::query_as::<_, Folder>(
            "SELECT f.id, f.parent_id, f.name, f.created_at, f.updated_at,
                    CASE WHEN EXISTS (SELECT 1 FROM folders AS child WHERE child.parent_id = f.id)
                           OR EXISTS (SELECT 1 FROM media WHERE media.folder_id = f.id)
                         THEN 1 ELSE 0 END AS has_contents
             FROM folders AS f
             ORDER BY f.parent_id, f.sort_order, lower(f.name), f.created_at, f.id",
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
        let result = sqlx::query(
            "INSERT INTO folders (id, parent_id, name, sort_order)
             VALUES (?, ?, ?, (
                 SELECT COALESCE(MAX(sort_order), -1) + 1
                 FROM folders WHERE parent_id IS ?
             ))",
        )
        .bind(id)
        .bind(parent_id)
        .bind(name)
        .bind(parent_id)
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
            "SELECT f.id, f.parent_id, f.name, f.created_at, f.updated_at,
                    CASE WHEN EXISTS (SELECT 1 FROM folders AS child WHERE child.parent_id = f.id)
                           OR EXISTS (SELECT 1 FROM media WHERE media.folder_id = f.id)
                         THEN 1 ELSE 0 END AS has_contents
             FROM folders AS f WHERE f.id = ?",
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

    pub async fn reorder(
        &self,
        id: &str,
        target_id: &str,
        placement: ReorderPlacement,
    ) -> AppResult<()> {
        if id == target_id {
            return Err(AppError::Validation(
                "a folder cannot be moved onto itself".into(),
            ));
        }

        let mut transaction = self.pool.begin().await?;
        let folders = sqlx::query_as::<_, Folder>(
            "SELECT f.id, f.parent_id, f.name, f.created_at, f.updated_at,
                    CASE WHEN EXISTS (SELECT 1 FROM folders AS child WHERE child.parent_id = f.id)
                           OR EXISTS (SELECT 1 FROM media WHERE media.folder_id = f.id)
                         THEN 1 ELSE 0 END AS has_contents
             FROM folders AS f
             ORDER BY f.parent_id, f.sort_order, lower(f.name), f.created_at, f.id",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let moved = folders
            .iter()
            .find(|folder| folder.id == id)
            .ok_or(AppError::NotFound)?;
        let target = folders
            .iter()
            .find(|folder| folder.id == target_id)
            .ok_or(AppError::NotFound)?;
        if moved.parent_id != target.parent_id {
            return Err(AppError::Validation(
                "folders can only be reordered within the same parent".into(),
            ));
        }

        let mut siblings: Vec<&str> = folders
            .iter()
            .filter(|folder| folder.parent_id == moved.parent_id && folder.id != id)
            .map(|folder| folder.id.as_str())
            .collect();
        let target_index = siblings
            .iter()
            .position(|sibling_id| *sibling_id == target_id)
            .ok_or_else(|| AppError::Validation("target folder is not a sibling".into()))?;
        let insert_index = target_index
            + match placement {
                ReorderPlacement::Before => 0,
                ReorderPlacement::After => 1,
            };
        siblings.insert(insert_index, id);

        for (index, sibling_id) in siblings.into_iter().enumerate() {
            sqlx::query("UPDATE folders SET sort_order = ? WHERE id = ?")
                .bind(index as i64)
                .bind(sibling_id)
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        let parent_id: Option<String> =
            sqlx::query_scalar("SELECT parent_id FROM folders WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(AppError::NotFound)?;

        let children: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, name FROM folders WHERE parent_id = ? ORDER BY sort_order, lower(name), id",
        )
        .bind(id)
        .fetch_all(&mut *transaction)
        .await?;
        let mut next_sort_order: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM folders WHERE parent_id IS ?",
        )
        .bind(parent_id.as_deref())
        .fetch_one(&mut *transaction)
        .await?;

        for (child_id, name) in children {
            let mut candidate = name.clone();
            let mut suffix = 2;
            loop {
                let name_in_use: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM folders WHERE parent_id IS ? AND lower(name) = lower(?)",
                )
                .bind(parent_id.as_deref())
                .bind(&candidate)
                .fetch_one(&mut *transaction)
                .await?;
                if name_in_use == 0 {
                    break;
                }
                candidate = name_with_suffix(&name, suffix);
                suffix += 1;
            }

            sqlx::query(
                "UPDATE folders
                 SET parent_id = ?, name = ?, sort_order = ?, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?",
            )
            .bind(parent_id.as_deref())
            .bind(candidate)
            .bind(next_sort_order)
            .bind(child_id)
            .execute(&mut *transaction)
            .await?;
            next_sort_order += 1;
        }

        sqlx::query(
            "UPDATE media SET folder_id = ?, updated_at = CURRENT_TIMESTAMP WHERE folder_id = ?",
        )
        .bind(parent_id.as_deref())
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE downloads SET folder_id = ? WHERE folder_id = ?")
            .bind(parent_id.as_deref())
            .bind(id)
            .execute(&mut *transaction)
            .await?;

        let result = sqlx::query("DELETE FROM folders WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn begin_delete_contents(&self, id: &str) -> AppResult<FolderDeletePlan> {
        let mut transaction = self.pool.begin().await?;
        // Acquire SQLite's writer lock before inspecting downloads or media. A download
        // cannot finish into this folder while its files are being quarantined.
        let locked = sqlx::query("UPDATE folders SET updated_at = updated_at WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        if locked.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }

        let folder_ids: Vec<String> = sqlx::query_scalar(
            "WITH RECURSIVE descendants(id, depth) AS (
                 SELECT id, 0 FROM folders WHERE id = ?
                 UNION ALL
                 SELECT child.id, parent.depth + 1 FROM folders AS child
                 JOIN descendants AS parent ON child.parent_id = parent.id
             )
             SELECT id FROM descendants ORDER BY depth, id",
        )
        .bind(id)
        .fetch_all(&mut *transaction)
        .await?;

        let active_downloads: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM downloads
             WHERE folder_id IN (
                 WITH RECURSIVE descendants(id) AS (
                     SELECT id FROM folders WHERE id = ?
                     UNION ALL
                     SELECT child.id FROM folders AS child
                     JOIN descendants AS parent ON child.parent_id = parent.id
                 ) SELECT id FROM descendants
             )
             AND status IN ('queued', 'analyzing', 'downloading', 'processing', 'verifying', 'finalizing')",
        )
        .bind(id)
        .fetch_one(&mut *transaction)
        .await?;
        if active_downloads > 0 {
            return Err(AppError::Conflict(
                "cancel downloads targeting this folder before deleting its contents".into(),
            ));
        }

        let media_paths: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT file_path, thumbnail_path FROM media
             WHERE folder_id IN (
                 WITH RECURSIVE descendants(id) AS (
                     SELECT id FROM folders WHERE id = ?
                     UNION ALL
                     SELECT child.id FROM folders AS child
                     JOIN descendants AS parent ON child.parent_id = parent.id
                 ) SELECT id FROM descendants
             )",
        )
        .bind(id)
        .fetch_all(&mut *transaction)
        .await?;

        Ok(FolderDeletePlan {
            transaction,
            folder_ids,
            media_paths,
        })
    }

    pub async fn commit_delete_contents(&self, mut plan: FolderDeletePlan) -> AppResult<()> {
        for folder_id in &plan.folder_ids {
            sqlx::query("UPDATE downloads SET folder_id = NULL WHERE folder_id = ?")
                .bind(folder_id)
                .execute(&mut *plan.transaction)
                .await?;
            sqlx::query("DELETE FROM media WHERE folder_id = ?")
                .bind(folder_id)
                .execute(&mut *plan.transaction)
                .await?;
        }
        // SQLite RESTRICT constraints require descendants to be removed before parents.
        for folder_id in plan.folder_ids.iter().rev() {
            sqlx::query("DELETE FROM folders WHERE id = ?")
                .bind(folder_id)
                .execute(&mut *plan.transaction)
                .await?;
        }
        plan.transaction.commit().await?;
        Ok(())
    }
}

fn name_with_suffix(name: &str, number: usize) -> String {
    let suffix = format!(" ({number})");
    let limit = 120usize.saturating_sub(suffix.chars().count());
    let base: String = name.chars().take(limit).collect();
    format!("{}{suffix}", base.trim_end())
}
