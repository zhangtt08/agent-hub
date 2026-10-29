use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentReport {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub logo: String,
    pub tint: String,
    pub mode: String,
    pub installed: bool,
    pub roots: Vec<String>,
    pub count: usize,
    pub bytes: u64,
    pub messages: u64,
    pub last_sync: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub agent: String,
    pub path: String,
    pub size: u64,
    pub mtime_ms: i64,
    pub mtime: String,
    pub created_at: String,
    pub title: String,
    pub project: String,
    pub project_path: String,
    pub messages: u64,
    pub compressed: bool,
    pub in_archive_dir: bool,
    pub archived: bool,
    pub archived_by_user: bool,
    pub readonly: bool,
    pub previewable: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub key: String,
    pub label: String,
    pub count: usize,
    pub bytes: u64,
    pub ids: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub conversations: usize,
    pub bytes: u64,
    pub messages: u64,
    pub installed: usize,
    pub with_sessions: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub running: bool,
    pub done: usize,
    pub total: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub scanning: Progress,
    pub scanned_at: Option<String>,
    pub duration_ms: u64,
    pub agents: Vec<AgentReport>,
    pub conversations: Vec<Conversation>,
    pub buckets: Vec<Bucket>,
    pub totals: Totals,
    pub data_dir: String,
    pub version: String,
    #[serde(default)]
    pub starred: Vec<String>,
    #[serde(default)]
    pub indexing: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMessage {
    pub role: String,
    pub text: String,
    pub time: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub messages: Vec<PreviewMessage>,
    pub truncated: bool,
    pub hint: String,
    pub conversation: Conversation,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeleteError {
    pub id: String,
    pub error: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub deleted: Vec<String>,
    pub errors: Vec<DeleteError>,
    pub freed: u64,
    pub purge: bool,
    pub totals: Option<Totals>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoredState {
    pub version: u32,
    #[serde(default)]
    pub archived: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub deleted: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub starred: Vec<String>,
}
