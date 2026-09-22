//! Read-only local Wiki folders on top of the existing knowledge graph.

mod config;
mod files;
mod legacy;
mod migration;
mod query;
mod sync;
mod visibility;

pub(crate) use config::active_space_ids;
pub use config::{connect, spaces, WikiSpace};
pub use legacy::{legacy_view, replace_legacy};
pub use query::{documents, read_document, WikiDocument, WikiDocumentsResult, WikiReadDocument};
pub use sync::{sync, WikiSyncResult};
pub use visibility::active_node_ids;

const SOURCE_ID: &str = "wiki";
const CONFIG_ID: &str = "wiki";
const MAX_BODY_BYTES: u64 = 2 * 1024 * 1024;

#[cfg(test)]
mod tests;
