//! Strict v1 workflow plan model.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{graph::ValidatedGraph, policy};

pub const WORKFLOW_SCHEMA_VERSION: u32 = 1;
pub const MAX_SPEC_BYTES: usize = 256 * 1024;
pub const MAX_TASKS: usize = 100;
pub const MAX_EDGES: usize = 500;
pub const MAX_ATTEMPTS_PER_TASK: u32 = 3;
pub const MAX_CONCURRENT_TASKS: u32 = 3;

const MAX_ID_BYTES: usize = 64;
const MAX_TEXT_BYTES: usize = 12_000;
const MAX_ITEM_BYTES: usize = 2_000;
const MAX_PHASES: usize = MAX_TASKS;
const MAX_WRITE_PATHS: usize = 64;
const MAX_STEP_RESOURCES: usize = 32;
const MAX_INPUT_ARTIFACTS: usize = 64;
const MAX_OUTPUT_PATHS: usize = 64;
const MAX_CHECKS: usize = 32;
const MAX_MANUAL_ACCEPTANCE: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSpec {
    pub schema_version: u32,
    pub project_ref: String,
    pub base_commit: String,
    pub phases: Vec<PhaseSpec>,
    pub tasks: Vec<TaskSpec>,
    pub edges: Vec<EdgeSpec>,
    pub final_task_id: String,
    pub execution_profile_id: String,
    pub limits: WorkflowLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseSpec {
    pub id: String,
    pub name: String,
    pub order: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub id: String,
    pub phase_id: String,
    pub kind: TaskKind,
    pub objective: String,
    pub write_paths: Vec<String>,
    pub resource_requests_by_step: BTreeMap<StepKind, Vec<ResourceRequest>>,
    pub input_artifacts: Vec<InputArtifactRef>,
    pub output_contract: OutputContract,
    pub checks: Vec<CheckSpec>,
    pub manual_acceptance: Vec<String>,
    pub retry_policy: RetryPolicy,
    #[serde(default)]
    pub command_profile_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Agent,
    Command,
    Integration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Execute,
    Verify,
    Integrate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    SharedRead,
    ExclusiveWrite,
    Capacity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRequest {
    pub resource_id: String,
    pub mode: AccessMode,
    pub units: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputArtifactRef {
    pub task_id: String,
    pub artifact: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    pub include_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSpec {
    pub id: String,
    pub profile_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub auto_retry_transient: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowLimits {
    pub max_tasks: u32,
    pub max_edges: u32,
    pub max_attempts_per_task: u32,
    #[serde(default = "default_max_concurrent_tasks")]
    pub max_concurrent_tasks: u32,
}

fn default_max_concurrent_tasks() -> u32 {
    2
}

impl WorkflowSpec {
    pub fn parse_json(json: &str) -> Result<Self, String> {
        if json.len() > MAX_SPEC_BYTES {
            return Err(format!("workflow spec exceeds {MAX_SPEC_BYTES} bytes"));
        }
        let spec: Self = serde_json::from_str(json)
            .map_err(|error| format!("invalid workflow spec JSON: {error}"))?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<ValidatedGraph, String> {
        if self.schema_version != WORKFLOW_SCHEMA_VERSION {
            return Err(format!(
                "unsupported workflow schema_version: {}",
                self.schema_version
            ));
        }
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("failed to serialize workflow spec: {error}"))?;
        if encoded.len() > MAX_SPEC_BYTES {
            return Err(format!("workflow spec exceeds {MAX_SPEC_BYTES} bytes"));
        }
        super::policy::validate_project_ref(&self.project_ref)?;
        validate_git_sha("base_commit", &self.base_commit)?;
        validate_id("final_task_id", &self.final_task_id)?;
        validate_id("execution_profile_id", &self.execution_profile_id)?;
        validate_limits(&self.limits)?;

        if self.phases.is_empty() || self.phases.len() > MAX_PHASES {
            return Err(format!("phases must contain 1..={MAX_PHASES} items"));
        }
        if self.tasks.is_empty() || self.tasks.len() > MAX_TASKS {
            return Err(format!("tasks must contain 1..={MAX_TASKS} items"));
        }
        if self.edges.len() > MAX_EDGES {
            return Err(format!("edges must contain at most {MAX_EDGES} items"));
        }
        if self.tasks.len() > self.limits.max_tasks as usize {
            return Err("tasks exceed limits.max_tasks".into());
        }
        if self.edges.len() > self.limits.max_edges as usize {
            return Err("edges exceed limits.max_edges".into());
        }

        let mut phase_ids: BTreeSet<String> = BTreeSet::new();
        let mut phase_orders = BTreeSet::new();
        for phase in &self.phases {
            validate_id("phase.id", &phase.id)?;
            validate_non_empty("phase.name", &phase.name, MAX_ITEM_BYTES)?;
            if !phase_ids.insert(phase.id.clone()) {
                return Err(format!("duplicate phase id: {}", phase.id));
            }
            if !phase_orders.insert(phase.order) {
                return Err(format!("duplicate phase order: {}", phase.order));
            }
        }

        let mut task_ids: BTreeSet<String> = BTreeSet::new();
        for task in &self.tasks {
            validate_task(task, &phase_ids, self.limits.max_attempts_per_task)?;
            if !task_ids.insert(task.id.clone()) {
                return Err(format!("duplicate task id: {}", task.id));
            }
        }

        let mut edge_ids = BTreeSet::new();
        for edge in &self.edges {
            validate_id("edge.from", &edge.from)?;
            validate_id("edge.to", &edge.to)?;
            if edge.from == edge.to {
                return Err(format!("self edge is not allowed: {}", edge.from));
            }
            if !task_ids.contains(&edge.from) || !task_ids.contains(&edge.to) {
                return Err(format!(
                    "edge references an unknown task: {} -> {}",
                    edge.from, edge.to
                ));
            }
            if !edge_ids.insert((edge.from.as_str(), edge.to.as_str())) {
                return Err(format!("duplicate edge: {} -> {}", edge.from, edge.to));
            }
        }

        for task in &self.tasks {
            for input in &task.input_artifacts {
                if !task_ids.contains(&input.task_id) {
                    return Err(format!(
                        "task {} references unknown input task {}",
                        task.id, input.task_id
                    ));
                }
                if input.task_id == task.id {
                    return Err(format!(
                        "task {} cannot use its own output as input",
                        task.id
                    ));
                }
                if !edge_ids.contains(&(input.task_id.as_str(), task.id.as_str())) {
                    return Err(format!(
                        "task {} input from {} requires a direct dependency edge",
                        task.id, input.task_id
                    ));
                }
            }
        }

        let final_task = self
            .tasks
            .iter()
            .find(|task| task.id == self.final_task_id)
            .ok_or_else(|| format!("final_task_id is not a task: {}", self.final_task_id))?;
        if final_task.kind != TaskKind::Integration {
            return Err("final_task_id must identify an integration task".into());
        }
        if final_task.checks.is_empty() {
            return Err("final integration task must include at least one mechanical check".into());
        }

        let graph = ValidatedGraph::build(self)?;
        if graph.topological_order().len() != self.tasks.len() {
            return Err("workflow graph contains a cycle".into());
        }
        let final_ancestors = graph.ancestors_of(&self.final_task_id);
        let disconnected: Vec<_> = task_ids
            .iter()
            .filter(|id| *id != &self.final_task_id && !final_ancestors.contains(*id))
            .cloned()
            .collect();
        if !disconnected.is_empty() {
            return Err(format!(
                "every non-final task must reach final_task_id; disconnected: {}",
                disconnected.join(", ")
            ));
        }
        Ok(graph)
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("failed to serialize workflow spec: {error}"))?;
        Ok(sha256_hex(&bytes))
    }

    /// Hashes execution-affecting fields of one task. Phase display fields are excluded.
    pub fn task_execution_hash(&self, task_id: &str) -> Result<String, String> {
        self.validate()?;
        let task = self
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| format!("unknown task id: {task_id}"))?;
        let execution = TaskExecutionHashInput {
            project_ref: &self.project_ref,
            limits: &self.limits,
            base_commit: &self.base_commit,
            execution_profile_id: &self.execution_profile_id,
            id: &task.id,
            kind: task.kind,
            objective: &task.objective,
            write_paths: &task.write_paths,
            resource_requests_by_step: &task.resource_requests_by_step,
            input_artifacts: &task.input_artifacts,
            output_contract: &task.output_contract,
            checks: &task.checks,
            manual_acceptance: &task.manual_acceptance,
            retry_policy: &task.retry_policy,
            command_profile_id: &task.command_profile_id,
        };
        let bytes = serde_json::to_vec(&execution)
            .map_err(|error| format!("failed to serialize task execution hash input: {error}"))?;
        Ok(sha256_hex(&bytes))
    }
}

#[derive(Serialize)]
struct TaskExecutionHashInput<'a> {
    project_ref: &'a str,
    limits: &'a WorkflowLimits,
    base_commit: &'a str,
    execution_profile_id: &'a str,
    id: &'a str,
    kind: TaskKind,
    objective: &'a str,
    write_paths: &'a [String],
    resource_requests_by_step: &'a BTreeMap<StepKind, Vec<ResourceRequest>>,
    input_artifacts: &'a [InputArtifactRef],
    output_contract: &'a OutputContract,
    checks: &'a [CheckSpec],
    manual_acceptance: &'a [String],
    retry_policy: &'a RetryPolicy,
    command_profile_id: &'a Option<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_limits(limits: &WorkflowLimits) -> Result<(), String> {
    if !(1..=MAX_TASKS as u32).contains(&limits.max_tasks) {
        return Err(format!("limits.max_tasks must be 1..={MAX_TASKS}"));
    }
    if limits.max_edges > MAX_EDGES as u32 {
        return Err(format!("limits.max_edges must be 0..={MAX_EDGES}"));
    }
    if !(1..=MAX_ATTEMPTS_PER_TASK).contains(&limits.max_attempts_per_task) {
        return Err(format!(
            "limits.max_attempts_per_task must be 1..={MAX_ATTEMPTS_PER_TASK}"
        ));
    }
    if !(1..=MAX_CONCURRENT_TASKS).contains(&limits.max_concurrent_tasks) {
        return Err(format!(
            "limits.max_concurrent_tasks must be 1..={MAX_CONCURRENT_TASKS}"
        ));
    }
    Ok(())
}

fn validate_task(
    task: &TaskSpec,
    phase_ids: &BTreeSet<String>,
    max_attempts: u32,
) -> Result<(), String> {
    validate_id("task.id", &task.id)?;
    validate_id("task.phase_id", &task.phase_id)?;
    if !phase_ids.contains(&task.phase_id) {
        return Err(format!(
            "task {} references unknown phase {}",
            task.id, task.phase_id
        ));
    }
    validate_non_empty("task.objective", &task.objective, MAX_TEXT_BYTES)?;
    validate_string_items(
        "task.write_paths",
        &task.write_paths,
        MAX_WRITE_PATHS,
        |path| policy::validate_safe_path(path),
    )?;
    validate_resource_requests(&task.resource_requests_by_step)?;
    if task.input_artifacts.len() > MAX_INPUT_ARTIFACTS {
        return Err(format!(
            "task.input_artifacts must contain at most {MAX_INPUT_ARTIFACTS} items"
        ));
    }
    let mut input_ids = BTreeSet::new();
    for input in &task.input_artifacts {
        validate_id("input_artifacts.task_id", &input.task_id)?;
        validate_id("input_artifacts.artifact", &input.artifact)?;
        if !input_ids.insert((&input.task_id, &input.artifact)) {
            return Err(format!("task {} has a duplicate input artifact", task.id));
        }
    }
    validate_output_contract(&task.output_contract)?;
    validate_checks(&task.checks)?;
    validate_string_items(
        "task.manual_acceptance",
        &task.manual_acceptance,
        MAX_MANUAL_ACCEPTANCE,
        |value| validate_non_empty("manual_acceptance item", value, MAX_ITEM_BYTES),
    )?;
    if !(1..=max_attempts).contains(&task.retry_policy.max_attempts) {
        return Err(format!(
            "task {} retry_policy.max_attempts must be 1..={max_attempts}",
            task.id
        ));
    }
    if let Some(profile_id) = &task.command_profile_id {
        validate_id("task.command_profile_id", profile_id)?;
    }
    if task.kind == TaskKind::Command && task.command_profile_id.is_none() {
        return Err(format!(
            "command task {} requires command_profile_id",
            task.id
        ));
    }
    Ok(())
}

fn validate_resource_requests(
    by_step: &BTreeMap<StepKind, Vec<ResourceRequest>>,
) -> Result<(), String> {
    for (step, requests) in by_step {
        if requests.len() > MAX_STEP_RESOURCES {
            return Err(format!(
                "{step:?} resources exceed {MAX_STEP_RESOURCES} items"
            ));
        }
        let mut ids = BTreeSet::new();
        for request in requests {
            policy::validate_resource_id(&request.resource_id)?;
            if request.units == 0 {
                return Err("resource request units must be greater than zero".into());
            }
            if request.mode != AccessMode::Capacity && request.units != 1 {
                return Err("shared_read and exclusive_write requests must use one unit".into());
            }
            if !ids.insert(&request.resource_id) {
                return Err(format!(
                    "duplicate resource request for step {step:?}: {}",
                    request.resource_id
                ));
            }
        }
    }
    Ok(())
}

fn validate_output_contract(contract: &OutputContract) -> Result<(), String> {
    if contract.include_paths.is_empty() {
        return Err("output_contract.include_paths must not be empty".into());
    }
    validate_string_items(
        "output_contract.include_paths",
        &contract.include_paths,
        MAX_OUTPUT_PATHS,
        |path| policy::validate_safe_path(path),
    )?;
    validate_string_items(
        "output_contract.exclude_paths",
        &contract.exclude_paths,
        MAX_OUTPUT_PATHS,
        |path| policy::validate_safe_path(path),
    )?;
    Ok(())
}

fn validate_checks(checks: &[CheckSpec]) -> Result<(), String> {
    if checks.len() > MAX_CHECKS {
        return Err(format!("checks must contain at most {MAX_CHECKS} items"));
    }
    let mut ids = BTreeSet::new();
    for check in checks {
        validate_id("check.id", &check.id)?;
        validate_id("check.profile_id", &check.profile_id)?;
        if !ids.insert(&check.id) {
            return Err(format!("duplicate check id: {}", check.id));
        }
    }
    Ok(())
}

fn validate_string_items<F>(
    name: &str,
    items: &[String],
    max_items: usize,
    validate: F,
) -> Result<(), String>
where
    F: Fn(&str) -> Result<(), String>,
{
    if items.len() > max_items {
        return Err(format!("{name} must contain at most {max_items} items"));
    }
    let mut seen = BTreeSet::new();
    for item in items {
        validate(item).map_err(|error| format!("{name}: {error}"))?;
        if !seen.insert(item) {
            return Err(format!("{name} must not contain duplicates"));
        }
    }
    Ok(())
}

fn validate_id(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') && index > 0
        })
        || !value.as_bytes()[0].is_ascii_alphanumeric()
    {
        return Err(format!(
            "{field} must be an ASCII identifier up to {MAX_ID_BYTES} bytes"
        ));
    }
    Ok(())
}

fn validate_git_sha(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "{field} must be an exact 40-character lowercase Git SHA"
        ));
    }
    Ok(())
}

fn validate_non_empty(field: &str, value: &str, max_bytes: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.trim() != value
        || value.len() > max_bytes
        || value.contains('\0')
    {
        return Err(format!(
            "{field} must be trimmed and contain 1..={max_bytes} bytes"
        ));
    }
    Ok(())
}
