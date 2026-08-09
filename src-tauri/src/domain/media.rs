use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Type};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum MediaKind {
    Video,
    Audio,
}

#[derive(Clone, Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: String,
    pub folder_id: Option<String>,
    pub title: String,
    pub media_type: MediaKind,
    pub source_url: String,
    pub source_platform: String,
    pub source_id: String,
    pub creator: Option<String>,
    pub file_path: String,
    pub thumbnail_path: Option<String>,
    pub container: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub file_size: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveMediaInput {
    pub id: String,
    pub folder_id: Option<String>,
}
