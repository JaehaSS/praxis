//! Durable review annotations and conversation resend formatting.

use serde::{Deserialize, Serialize};

mod rematch;
mod resend;
mod store;

pub use rematch::{rematch, RematchedAnnotation};
pub use resend::{build_resend_items, format_resend, quote_line, ResendItem};
pub use store::{
    create_draft, list_by_ids, list_by_task, mark_draft, mark_sent, migrate, update_draft_body,
};

pub mod status {
    pub const DRAFT: &str = "draft";
    pub const SENT: &str = "sent";
    pub const RESOLVED: &str = "resolved";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ReviewAnnotation {
    pub id: String,
    pub task_id: i64,
    pub hunk_id: String,
    pub path: String,
    pub line: i64,
    pub side: String,
    pub body_md: String,
    pub status: String,
    pub created_at: i64,
}

#[cfg(test)]
mod tests;
