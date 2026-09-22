/** Shared JSON v1 contract for Runner workflow plans. Field names stay snake_case to match Rust. */
export interface WorkflowSpec {
  schema_version: 1;
  project_ref: string;
  base_commit: string;
  phases: PhaseSpec[];
  tasks: TaskSpec[];
  edges: EdgeSpec[];
  final_task_id: string;
  execution_profile_id: string;
  limits: WorkflowLimits;
}

export interface PhaseSpec {
  id: string;
  name: string;
  order: number;
}

export interface TaskSpec {
  id: string;
  phase_id: string;
  kind: TaskKind;
  objective: string;
  write_paths: string[];
  resource_requests_by_step: Partial<Record<StepKind, ResourceRequest[]>>;
  input_artifacts: InputArtifactRef[];
  output_contract: OutputContract;
  checks: CheckSpec[];
  /** Human-reviewed criteria; these never count as a mechanical check. */
  manual_acceptance: string[];
  retry_policy: RetryPolicy;
  /** Required for `command` tasks. */
  command_profile_id?: string | null;
}

export type TaskKind = "agent" | "command" | "integration";
export type StepKind = "execute" | "verify" | "integrate";
export type AccessMode = "shared_read" | "exclusive_write" | "capacity";

export interface ResourceRequest {
  resource_id: string;
  mode: AccessMode;
  units: number;
}

export interface InputArtifactRef {
  task_id: string;
  artifact: string;
}

export interface OutputContract {
  include_paths: string[];
  exclude_paths: string[];
}

export interface CheckSpec {
  id: string;
  profile_id: string;
}

export interface RetryPolicy {
  max_attempts: number;
  auto_retry_transient: boolean;
}

export interface EdgeSpec {
  from: string;
  to: string;
}

export interface WorkflowLimits {
  max_tasks: number;
  max_edges: number;
  max_attempts_per_task: number;
  /** Omitted JSON defaults to two, matching the initial Runner capacity. */
  max_concurrent_tasks?: number;
}
