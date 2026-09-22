//! Fail-closed memory projection journal and filesystem orchestration.

mod apply;
mod preparation;
mod verification;

pub use apply::inject_into_worktree;
pub use verification::{
    verify_task_projection, verify_task_projection_for_start, ProjectionStartReceipt,
};
