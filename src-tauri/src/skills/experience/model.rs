use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessName {
    WorkflowHarness,
    LoopEngineering,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Vendor {
    Claude,
    Codex,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    Project,
    Global,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Host {
    Local,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub host: Host,
    pub harness: HarnessName,
    pub vendor: Vendor,
    pub scope: Scope,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub host: Host,
    pub project_key: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DocumentOwner {
    Skill { source: SourceRef },
    Project { project: ProjectRef },
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReadError {
    NotFound,
    PermissionDenied,
    UnsafePath,
    NotRegular,
    Oversized,
    InvalidUtf8,
    Changed,
    IoError,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDescriptor {
    pub key: String,
    pub display_path: String,
    pub relation: Relation,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Relation {
    HarnessOwned,
    ProjectReference,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExperienceDocument {
    pub owner: DocumentOwner,
    pub document_key: String,
    pub path: String,
    pub content_hash: String,
    pub observed_at: String,
    pub generated: bool,
    pub source_resolution: SourceResolution,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceResolution {
    KnownSet,
    Unconfirmed,
    NotApplicable,
}
