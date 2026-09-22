use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CodeLocationInput {
    pub relative_path: String,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LocalDocumentInput {
    pub relative_path: String,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExternalDocumentInput {
    pub url: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct EvidenceRecord {
    pub id: i64,
    pub memory_id: i64,
    pub version: i64,
    pub kind: String,
    pub locator_json: String,
    pub snapshot_hash: Option<String>,
    pub status: String,
    pub observed_at: i64,
    pub checked_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RevalidationReport {
    pub memory_id: i64,
    pub version: i64,
    pub statuses: Vec<String>,
    pub check_ids: Vec<i64>,
    pub stale: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeLocator {
    pub schema_version: u32,
    pub canonical_repository: String,
    pub relative_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub commit_oid: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalDocumentLocator {
    pub schema_version: u32,
    pub canonical_repository: String,
    pub relative_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalDocumentLocator {
    pub schema_version: u32,
    pub url: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Observation {
    pub evidence: EvidenceRecord,
    pub status: String,
    pub observed_hash: Option<String>,
}
