use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolUpdateState {
    NotChecked,
    Current,
    UpdateAvailable,
    CheckFailed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolVersionStatus {
    pub id: String,
    pub name: String,
    pub installed_version: String,
    pub latest_version: Option<String>,
    pub state: ToolUpdateState,
    pub integrity_verified: bool,
    pub update_supported: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatusReport {
    pub checked_at: Option<String>,
    pub tools: Vec<ToolVersionStatus>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolUpdateResult {
    pub updated_tools: Vec<String>,
    pub restart_required: bool,
}
