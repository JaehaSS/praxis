//! Versioned workflow plan contract and deterministic DAG operations.

pub mod artifacts;
pub mod events;
mod graph;
pub mod inputs;
pub mod lifecycle;
mod model;
pub mod policy;
pub mod query;
pub mod resources;
mod revisions;
pub mod scheduler;
mod schema;
pub mod store;
pub mod verification;

pub use graph::ValidatedGraph;
pub use model::{
    AccessMode, CheckSpec, EdgeSpec, InputArtifactRef, OutputContract, PhaseSpec, ResourceRequest,
    RetryPolicy, StepKind, TaskKind, TaskSpec, WorkflowLimits, WorkflowSpec,
    WORKFLOW_SCHEMA_VERSION,
};
