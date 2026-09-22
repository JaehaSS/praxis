use super::{DocumentDescriptor, ExperienceDocument, ProjectRef, ReadError, SourceRef};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SourceListing {
    NotInstalled {
        source: SourceRef,
    },
    Empty {
        source: SourceRef,
    },
    Error {
        source: SourceRef,
        error: ReadError,
    },
    Ready {
        source: SourceRef,
        documents: Vec<DocumentDescriptor>,
        limited: bool,
        inspected_entries: usize,
    },
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ProjectListing {
    Empty {
        project: ProjectRef,
    },
    Error {
        project: ProjectRef,
        error: ReadError,
    },
    Ready {
        project: ProjectRef,
        documents: Vec<DocumentDescriptor>,
        limited: bool,
        inspected_entries: usize,
    },
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UnsupportedReason {
    SecureReadUnavailable,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RequestError {
    UnregisteredProject,
    InvalidRequest,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ListResult {
    Unsupported {
        reason: UnsupportedReason,
    },
    Error {
        error: RequestError,
    },
    Ready {
        observed_at: String,
        sources: Vec<SourceListing>,
        project: ProjectListing,
    },
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ReadResult {
    Ready { document: ExperienceDocument },
    Unsupported { reason: UnsupportedReason },
    Error { error: ReadErrorOrRequest },
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ReadErrorOrRequest {
    Read(ReadError),
    Request(RequestError),
}
