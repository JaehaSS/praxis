use serde::Serialize;

pub const SCHEMA_VERSION: u8 = 1;
pub const MAX_FILES: usize = 5_000;
pub const MAX_SYMBOLS: usize = 200;
pub const MAX_RELATIONS: usize = 100;
pub const MAX_PAGE_BYTES: usize = 256 * 1024;
pub const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;
/// Rendered pages are held until every target passes conflict preflight.
pub const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
/// Existing page bytes are retained to detect edits between preflight and rename.
pub const MAX_PREIMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Graph rows are bounded before batch materialization.
pub const MAX_GRAPH_NODE_ROWS: usize = MAX_FILES * MAX_SYMBOLS;
pub const MAX_GRAPH_EDGE_ROWS: usize = MAX_FILES * MAX_RELATIONS;
pub const MAX_GRAPH_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeWikiStatus {
    pub graph_state: String,
    pub index_path: String,
    pub index_state: CodeWikiPageState,
    pub modules: Vec<CodeWikiModule>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeWikiModule {
    pub source_path: String,
    pub page_path: String,
    pub state: CodeWikiPageState,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CodeWikiPageState {
    Missing,
    Ready,
    Stale,
    Conflict,
    Orphaned,
}

impl CodeWikiPageState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Ready => "ready",
            Self::Stale => "stale",
            Self::Conflict => "conflict",
            Self::Orphaned => "orphaned",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Metadata {
    pub kind: String,
    pub source_path: Option<String>,
    pub source_hash: Option<String>,
    pub source_fingerprint: String,
    pub run_id: i64,
    pub generated_at: i64,
    pub checksum: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(super) struct NodeRow {
    pub rel_path: String,
    pub name: String,
    pub kind: i64,
    pub container: Option<String>,
    pub sel_start_line: i64,
    pub sel_start_char: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(super) struct EdgeRow {
    pub src_path: String,
    pub src_name: String,
    pub src_line: i64,
    pub dst_path: String,
    pub dst_name: String,
    pub dst_line: i64,
}
