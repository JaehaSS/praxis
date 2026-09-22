//! Backend-observed, immutable evidence and source revalidation.

mod model;
mod revalidate;
pub(crate) mod scoped_file;
mod source_snapshot;
mod store;

pub use model::{
    CodeLocationInput, EvidenceRecord, ExternalDocumentInput, LocalDocumentInput,
    RevalidationReport,
};
pub(crate) use revalidate::{approve_memory, confirm_and_approve_memory};
pub use revalidate::{revalidate_memories, revalidate_memory, revalidate_scope};
pub use store::{add_code_location, add_external_document, add_local_document, list_evidence};

/// Test evidence remains fail-closed until an enforceable isolation provider exists.
pub async fn add_test_run(
    _pool: &sqlx::SqlitePool,
    _memory_id: i64,
    _profile_id: i64,
    _now: i64,
) -> anyhow::Result<i64> {
    anyhow::bail!(
        "test_run evidence is unsupported until a trusted execution isolation provider is available"
    )
}
