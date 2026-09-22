import type { EdgeSpec, WorkflowSpec } from "./types";

export interface PlanIssue {
  path: string;
  message: string;
}

export interface PlanValidation {
  spec: WorkflowSpec | null;
  issues: PlanIssue[];
}

export function parseWorkflowPlanJson(source: string): PlanValidation {
  if (utf8Bytes(source) > 256 * 1024) return { spec: null, issues: [{ path: "$", message: "계획은 256KiB를 넘을 수 없습니다." }] };
  try { return validateWorkflowPlan(JSON.parse(source)); }
  catch { return { spec: null, issues: [{ path: "$", message: "JSON 형식이 올바르지 않습니다." }] }; }
}

const identifier = (value: unknown): value is string =>
  typeof value === "string" && value.trim().length > 0;

/** User text never becomes Mermaid syntax: quotes, brackets and line breaks are encoded first. */
export function mermaidLabel(value: string): string {
  return value.replace(/[&"\\\n\r\[\]{}<>]/g, (character) => {
    const code = character.charCodeAt(0).toString(16).padStart(2, "0");
    return `&#x${code};`;
  });
}

export function workflowMermaid(spec: WorkflowSpec): string {
  const lines = ["flowchart LR"];
  const nodeIds = new Map(spec.tasks.map((task, index) => [task.id, `n${index}`]));
  for (const task of spec.tasks) {
    lines.push(`  ${nodeIds.get(task.id)}[\"${mermaidLabel(task.id)} · ${mermaidLabel(task.objective)}\"]`);
  }
  for (const edge of spec.edges) lines.push(`  ${nodeIds.get(edge.from)} --> ${nodeIds.get(edge.to)}`);
  return lines.join("\n");
}

export function validateWorkflowPlan(value: unknown): PlanValidation {
  try { return validateWorkflowPlanInner(value); }
  catch { return { spec: null, issues: [{ path: "$", message: "계획 형식을 읽을 수 없습니다." }] }; }
}

function validateWorkflowPlanInner(value: unknown): PlanValidation {
  const issues: PlanIssue[] = [];
  if (!isRecord(value)) return { spec: null, issues: [{ path: "$", message: "JSON 객체여야 합니다." }] };
  if (utf8Bytes(JSON.stringify(value)) > 256 * 1024) return { spec: null, issues: [{ path: "$", message: "계획은 256KiB를 넘을 수 없습니다." }] };
  const spec = value as Partial<WorkflowSpec>;
  if (spec.schema_version !== 1) issues.push({ path: "schema_version", message: "지원하는 형식 버전은 1입니다." });
  for (const field of ["project_ref", "base_commit", "final_task_id", "execution_profile_id"] as const) {
    if (!identifier(spec[field])) issues.push({ path: field, message: "비어 있을 수 없습니다." });
  }
  if (!validId(spec.final_task_id)) issues.push({ path: "final_task_id", message: "영문·숫자 ID여야 합니다." });
  if (!validId(spec.execution_profile_id)) issues.push({ path: "execution_profile_id", message: "영문·숫자 ID여야 합니다." });
  if (!safePath(spec.project_ref) || utf8Bytes(spec.project_ref) > 256 || !/^[A-Za-z0-9._/-]+$/.test(spec.project_ref)) issues.push({ path: "project_ref", message: "등록된 저장소 키 형식이어야 합니다." });
  if (!/^[0-9a-f]{40}$/.test(spec.base_commit ?? "")) issues.push({ path: "base_commit", message: "40자리 소문자 Git SHA여야 합니다." });
  if (!Array.isArray(spec.phases)) issues.push({ path: "phases", message: "배열이어야 합니다." });
  if (!Array.isArray(spec.tasks) || spec.tasks.length === 0) issues.push({ path: "tasks", message: "하나 이상의 작업이 필요합니다." });
  if (!Array.isArray(spec.edges)) issues.push({ path: "edges", message: "배열이어야 합니다." });
  if (!isRecord(spec.limits)) issues.push({ path: "limits", message: "실행 한도가 필요합니다." });
  if (issues.length > 0) return { spec: null, issues };

  const phases = spec.phases as WorkflowSpec["phases"];
  const tasks = spec.tasks as WorkflowSpec["tasks"];
  const edges = spec.edges as EdgeSpec[];
  if (phases.length > 100) issues.push({ path: "phases", message: "Phase는 100개까지입니다." });
  if (tasks.length > 100) issues.push({ path: "tasks", message: "작업은 100개까지입니다." });
  if (edges.length > 500) issues.push({ path: "edges", message: "연결은 500개까지입니다." });
  const phaseIds = new Set<string>();
  phases.forEach((phase, index) => {
    if (!validId(phase?.id) || (!identifier(phase?.name) || utf8Bytes(phase.name) > 2_000) || !Number.isInteger(phase?.order)) issues.push({ path: `phases[${index}]`, message: "id, 이름, 순서가 필요합니다." });
    else if (phaseIds.has(phase.id)) issues.push({ path: `phases[${index}].id`, message: "Phase ID가 중복됩니다." });
    else phaseIds.add(phase.id);
  });
  const taskIds = new Set<string>();
  tasks.forEach((task, index) => {
    if (!validId(task?.id)) issues.push({ path: `tasks[${index}].id`, message: "작업 ID가 필요합니다." });
    else if (taskIds.has(task.id)) issues.push({ path: `tasks[${index}].id`, message: "작업 ID가 중복됩니다." });
    else taskIds.add(task.id);
    if (!validId(task?.phase_id) || !phaseIds.has(task?.phase_id)) issues.push({ path: `tasks[${index}].phase_id`, message: "존재하는 Phase를 가리켜야 합니다." });
    if (!identifier(task?.objective) || utf8Bytes(task.objective) > 12_000) issues.push({ path: `tasks[${index}].objective`, message: "작업 설명이 필요합니다." });
    if (!task || !["agent", "command", "integration"].includes(task.kind)) issues.push({ path: `tasks[${index}].kind`, message: "작업 종류가 올바르지 않습니다." });
    for (const field of ["write_paths", "input_artifacts", "checks", "manual_acceptance"] as const) {
      if (!Array.isArray(task?.[field])) issues.push({ path: `tasks[${index}].${field}`, message: "배열이어야 합니다." });
    }
    if (!isRecord(task?.resource_requests_by_step)) issues.push({ path: `tasks[${index}].resource_requests_by_step`, message: "단계별 자원 요청이 필요합니다." });
    else validateStepResources(task.resource_requests_by_step, `tasks[${index}].resource_requests_by_step`, issues);
    if (!isRecord(task?.output_contract) || !Array.isArray(task.output_contract.include_paths) || !Array.isArray(task.output_contract.exclude_paths) || task.output_contract.include_paths.length === 0 || task.output_contract.include_paths.length > 64 || task.output_contract.exclude_paths.length > 64 || !task.output_contract.include_paths.every(safePath) || !task.output_contract.exclude_paths.every(safePath)) issues.push({ path: `tasks[${index}].output_contract`, message: "안전한 산출물 경로가 하나 이상 필요합니다." });
    if (!isRecord(task?.retry_policy) || !Number.isInteger(task.retry_policy.max_attempts) || task.retry_policy.max_attempts < 1 || task.retry_policy.max_attempts > 3 || (isRecord(spec.limits) && Number.isInteger(spec.limits.max_attempts_per_task) && task.retry_policy.max_attempts > spec.limits.max_attempts_per_task) || typeof task.retry_policy.auto_retry_transient !== "boolean") issues.push({ path: `tasks[${index}].retry_policy`, message: "재시도 정책이 올바르지 않습니다." });
    if (Array.isArray(task?.write_paths) && (!task.write_paths.every(safePath) || task.write_paths.length > 64)) issues.push({ path: `tasks[${index}].write_paths`, message: "안전한 변경 경로를 64개까지 입력할 수 있습니다." });
    if (Array.isArray(task?.input_artifacts) && (!task.input_artifacts.every((input) => isRecord(input) && validId(input.task_id) && validId(input.artifact)) || task.input_artifacts.length > 64)) issues.push({ path: `tasks[${index}].input_artifacts`, message: "입력 산출물 형식이 올바르지 않습니다." });
    if (Array.isArray(task?.checks) && (!task.checks.every((check) => isRecord(check) && validId(check.id) && validId(check.profile_id)) || task.checks.length > 32)) issues.push({ path: `tasks[${index}].checks`, message: "검사 형식이 올바르지 않습니다." });
    if (Array.isArray(task?.manual_acceptance) && (!task.manual_acceptance.every((item) => identifier(item) && utf8Bytes(item) <= 2_000) || task.manual_acceptance.length > 32)) issues.push({ path: `tasks[${index}].manual_acceptance`, message: "수동 확인 항목 형식이 올바르지 않습니다." });
    if (task?.kind === "command" && !identifier(task.command_profile_id)) issues.push({ path: `tasks[${index}].command_profile_id`, message: "명령 작업에는 명령 프로필이 필요합니다." });
  });
  const outgoing = new Map<string, string[]>();
  const incoming = new Map<string, string[]>();
  const edgeIds = new Set<string>();
  edges.forEach((edge, index) => {
    if (!taskIds.has(edge?.from) || !taskIds.has(edge?.to) || edge.from === edge.to) {
      issues.push({ path: `edges[${index}]`, message: "서로 다른 존재 작업을 연결해야 합니다." });
      return;
    }
    if (edgeIds.has(`${edge.from}\u0000${edge.to}`)) { issues.push({ path: `edges[${index}]`, message: "같은 연결을 두 번 넣을 수 없습니다." }); return; }
    edgeIds.add(`${edge.from}\u0000${edge.to}`);
    outgoing.set(edge.from, [...(outgoing.get(edge.from) ?? []), edge.to]);
    incoming.set(edge.to, [...(incoming.get(edge.to) ?? []), edge.from]);
  });
  const finalTask = tasks.find((task) => task.id === spec.final_task_id);
  if (!finalTask) issues.push({ path: "final_task_id", message: "존재하는 최종 작업을 가리켜야 합니다." });
  else if (finalTask.kind !== "integration" || finalTask.checks.length === 0) issues.push({ path: "final_task_id", message: "최종 작업은 검사가 있는 통합 작업이어야 합니다." });
  else {
    const reachesFinal = ancestorsOf(spec.final_task_id!, incoming);
    if ([...taskIds].some((id) => !reachesFinal.has(id))) issues.push({ path: "final_task_id", message: "모든 작업은 최종 작업으로 이어져야 합니다." });
  }
  const limits = spec.limits as WorkflowSpec["limits"];
  for (const field of ["max_tasks", "max_edges", "max_attempts_per_task"] as const) {
    if (!Number.isInteger(limits[field]) || limits[field] < (field === "max_edges" ? 0 : 1) || (field === "max_tasks" && limits[field] > 100) || (field === "max_edges" && limits[field] > 500) || (field === "max_attempts_per_task" && limits[field] > 3)) issues.push({ path: `limits.${field}`, message: "지원 범위의 정수여야 합니다." });
  }
  if (limits.max_concurrent_tasks != null && (!Number.isInteger(limits.max_concurrent_tasks) || limits.max_concurrent_tasks < 1 || limits.max_concurrent_tasks > 3)) issues.push({ path: "limits.max_concurrent_tasks", message: "동시 작업은 1~3개여야 합니다." });
  if (tasks.length > limits.max_tasks) issues.push({ path: "limits.max_tasks", message: "작업 수가 지정한 한도를 초과합니다." });
  if (edges.length > limits.max_edges) issues.push({ path: "limits.max_edges", message: "연결 수가 지정한 한도를 초과합니다." });
  if (hasCycle(taskIds, outgoing)) issues.push({ path: "edges", message: "순환 의존성은 허용되지 않습니다." });
  return { spec: issues.length === 0 ? spec as WorkflowSpec : null, issues };
}

function hasCycle(nodes: Set<string>, outgoing: Map<string, string[]>): boolean {
  const visiting = new Set<string>();
  const visited = new Set<string>();
  const visit = (node: string): boolean => {
    if (visiting.has(node)) return true;
    if (visited.has(node)) return false;
    visiting.add(node);
    if ((outgoing.get(node) ?? []).some(visit)) return true;
    visiting.delete(node);
    visited.add(node);
    return false;
  };
  return [...nodes].some(visit);
}

function ancestorsOf(finalId: string, incoming: Map<string, string[]>): Set<string> {
  const found = new Set<string>([finalId]);
  const todo = [finalId];
  while (todo.length > 0) {
    const current = todo.pop()!;
    for (const parent of incoming.get(current) ?? []) {
      if (!found.has(parent)) { found.add(parent); todo.push(parent); }
    }
  }
  return found;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validId(value: unknown): value is string { return typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$/.test(value); }
function safePath(value: unknown): value is string { return typeof value === "string" && value.length > 0 && utf8Bytes(value) <= 512 && value.trim() === value && !value.startsWith("/") && !/^[A-Za-z]:/.test(value) && !value.includes("\\") && !/[\0*?\[\]{}!]/.test(value) && !value.endsWith("/") && value.split("/").every((part) => part !== "" && part !== "." && part !== ".."); }
function utf8Bytes(value: string): number { return new TextEncoder().encode(value).byteLength; }
function safeResourceId(value: unknown): value is string { return typeof value === "string" && value.length > 0 && utf8Bytes(value) <= 256 && value.trim() === value && !value.includes("\0") && !/[\n\r\\*?\[\]{}]/.test(value); }
function validateStepResources(value: Record<string, unknown>, path: string, issues: PlanIssue[]): void {
  for (const [step, requests] of Object.entries(value)) {
    if (!(["execute", "verify", "integrate"] as string[]).includes(step) || !Array.isArray(requests) || requests.length > 32) { issues.push({ path, message: "실행·검사·통합 단계별 자원 요청만 32개까지 허용됩니다." }); continue; }
    const ids = new Set<string>();
    for (const request of requests) {
      const row = isRecord(request) ? request : null;
      const mode = row?.mode;
      const units = row?.units;
      if (!row || !safeResourceId(row.resource_id) || !(["shared_read", "exclusive_write", "capacity"] as string[]).includes(String(mode)) || !Number.isInteger(units) || (units as number) < 1 || (units as number) > 1024 || ((mode === "shared_read" || mode === "exclusive_write") && units !== 1) || ids.has(row.resource_id)) { issues.push({ path, message: "자원 ID, 모드, 단위 또는 중복 요청이 올바르지 않습니다." }); break; }
      ids.add(row.resource_id);
    }
  }
}
