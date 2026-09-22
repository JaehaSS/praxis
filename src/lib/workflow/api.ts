import type { WorkflowSpec } from "./types";
import type { WorkflowEvent } from "./state";
import { validateWorkflowPlan } from "./graph";

export type WorkflowRunState = "draft" | "running" | "paused" | "completed" | "failed" | "cancelled" | "quarantined";
export type WorkflowNodeState = "pending" | "ready" | "executing" | "verifying" | "verified" | "failed" | "cancelled" | "quarantined" | "awaiting_acceptance";

export interface WorkflowCapability { supported: boolean; reason: string | null; }
export interface WorkflowNodeView {
  id: string; phase_id: string; state: WorkflowNodeState; wait_reason?: string | null;
  wait_detail?: string | null; inputs?: string[]; write_paths?: string[]; resources?: string[];
  checks?: Array<{ id: string; state: string; detail?: string | null }>;
  attempts?: Array<{ id: string; number: number; state: string; detail?: string | null }>;
}
export interface WorkflowSnapshot {
  id: string; revision: number; state: WorkflowRunState; last_sequence: number; spec: WorkflowSpec; nodes: WorkflowNodeView[];
}
export interface WorkflowReceipt { request_id: string; revision: number; accepted: boolean; error?: string | null; approved_scope_hash?: string | null; workflow_id?: string | null; }
export interface WorkflowTransport {
  capability(): Promise<WorkflowCapability>;
  list(): Promise<WorkflowSnapshot[]>;
  get(id: string): Promise<WorkflowSnapshot>;
  events(id: string, after: number): Promise<WorkflowEvent[]>;
  create(spec: WorkflowSpec, requestId: string): Promise<WorkflowReceipt>;
  validate(id: string, revision: number, requestId: string): Promise<WorkflowReceipt>;
  action(id: string, action: "start" | "pause" | "resume" | "cancel", revision: number, requestId: string, approvedScopeHash?: string): Promise<WorkflowReceipt>;
}

export interface WorkflowJsonClient { request<T>(path: string, init?: RequestInit): Promise<T>; }

export function createWorkflowTransport(client: WorkflowJsonClient): WorkflowTransport {
  const unavailable = (error: unknown): WorkflowCapability => ({
    supported: false,
    reason: error instanceof Error && /\((404|422)\)/.test(error.message) ? "연결된 Runner가 Workflow를 아직 지원하지 않습니다." : "Workflow Runner를 확인할 수 없습니다.",
  });
  return {
    async capability() {
      try {
        // D8 has no separate capability route. A successful typed list is the feature gate;
        // older Runners return 404 and never receive a mutation request from this client.
        decodeSnapshots(await client.request<unknown>("/v1/workflows"));
        return { supported: true, reason: null };
      } catch (error) { return unavailable(error); }
    },
    list: async () => decodeSnapshots(await client.request<unknown>("/v1/workflows")),
    get: async (id) => decodeSnapshot(await client.request<unknown>(`/v1/workflows/${encodeURIComponent(id)}`)),
    events: async (id, after) => decodeEvents(await client.request<unknown>(`/v1/workflows/${encodeURIComponent(id)}/events?after=${after}`)),
    create: async (spec, request_id) => decodeReceipt(await client.request<unknown>("/v1/workflows", { method: "POST", body: JSON.stringify({ request_id, spec }) })),
    validate: async (id, revision, request_id) => decodeReceipt(await client.request<unknown>(`/v1/workflows/${encodeURIComponent(id)}/validate`, { method: "POST", body: JSON.stringify({ request_id, expected_revision: revision }) })),
    action: async (id, action, revision, request_id, approved_scope_hash) => decodeReceipt(await client.request<unknown>(`/v1/workflows/${encodeURIComponent(id)}/${action}`, { method: "POST", body: JSON.stringify({ request_id, expected_revision: revision, ...(approved_scope_hash ? { approved_scope_hash } : {}) }) })),
  };
}

/** Local desktop has no Workflow IPC. Keeping this explicit prevents accidental command invention. */
export const unsupportedWorkflowTransport: WorkflowTransport = {
  capability: async () => ({ supported: false, reason: "Workflow 실행은 연결된 Runner에서만 사용할 수 있습니다." }),
  list: unsupported,
  get: unsupported,
  events: unsupported,
  create: unsupported,
  validate: unsupported,
  action: unsupported,
};

function unsupported(): Promise<never> {
  return Promise.reject(new Error("Workflow 실행은 연결된 Runner 전용입니다."));
}

const RUN_STATES = new Set<WorkflowRunState>(["draft", "running", "paused", "completed", "failed", "cancelled", "quarantined"]);
const NODE_STATES = new Set<WorkflowNodeState>(["pending", "ready", "executing", "verifying", "verified", "failed", "cancelled", "quarantined", "awaiting_acceptance"]);
const protocolError = (message: string): never => { throw new Error(`Workflow 응답 형식 오류: ${message}`); };
const record = (value: unknown): Record<string, unknown> | null => typeof value === "object" && value !== null && !Array.isArray(value) ? value as Record<string, unknown> : null;
const text = (value: unknown): value is string => typeof value === "string";
const integer = (value: unknown): value is number => Number.isSafeInteger(value);

function decodeSnapshots(value: unknown): WorkflowSnapshot[] {
  if (!Array.isArray(value)) return protocolError("계획 목록");
  return value.map(decodeSnapshot);
}
function decodeSnapshot(value: unknown): WorkflowSnapshot {
  const row = record(value);
  if (!row || !text(row.id) || !integer(row.revision) || row.revision < 1 || !text(row.state) || !RUN_STATES.has(row.state as WorkflowRunState) || !integer(row.last_sequence) || row.last_sequence < 0 || !Array.isArray(row.nodes) || row.nodes.length > 100) return protocolError("계획 snapshot");
  const validated = validateWorkflowPlan(row.spec);
  if (!validated.spec) return protocolError("계획 spec");
  return { id: row.id, revision: row.revision, state: row.state as WorkflowRunState, last_sequence: row.last_sequence, spec: validated.spec, nodes: row.nodes.map(decodeNode) };
}
function decodeNode(value: unknown): WorkflowNodeView {
  const row = record(value);
  if (!row || !text(row.id) || !text(row.phase_id) || !text(row.state) || !NODE_STATES.has(row.state as WorkflowNodeState) || !optionalText(row.wait_reason) || !optionalText(row.wait_detail) || !optionalTextArray(row.inputs, 64) || !optionalTextArray(row.write_paths, 64) || !optionalTextArray(row.resources, 64) || !optionalChecks(row.checks) || !optionalAttempts(row.attempts)) return protocolError("작업 snapshot");
  return { id: row.id, phase_id: row.phase_id, state: row.state as WorkflowNodeState, ...(row.wait_reason === undefined ? {} : { wait_reason: row.wait_reason as string | null }), ...(row.wait_detail === undefined ? {} : { wait_detail: row.wait_detail as string | null }), ...(row.inputs === undefined ? {} : { inputs: row.inputs as string[] }), ...(row.write_paths === undefined ? {} : { write_paths: row.write_paths as string[] }), ...(row.resources === undefined ? {} : { resources: row.resources as string[] }), ...(row.checks === undefined ? {} : { checks: row.checks as WorkflowNodeView["checks"] }), ...(row.attempts === undefined ? {} : { attempts: row.attempts as WorkflowNodeView["attempts"] }) };
}
function decodeEvents(value: unknown): WorkflowEvent[] {
  if (!Array.isArray(value) || value.length > 1000) return protocolError("이벤트 목록");
  return value.map((item) => { const row = record(item); if (!row || !integer(row.sequence) || row.sequence < 0 || !text(row.kind) || !optionalText(row.detail)) return protocolError("이벤트"); return { sequence: row.sequence, kind: row.kind, ...(row.detail === undefined ? {} : { detail: row.detail as string | null }) }; });
}
function decodeReceipt(value: unknown): WorkflowReceipt {
  const row = record(value);
  if (!row || !text(row.request_id) || !integer(row.revision) || row.revision < 1 || typeof row.accepted !== "boolean" || !optionalText(row.error) || !optionalText(row.approved_scope_hash) || !optionalText(row.workflow_id)) return protocolError("요청 영수증");
  return { request_id: row.request_id, revision: row.revision, accepted: row.accepted, ...(row.error === undefined ? {} : { error: row.error as string | null }), ...(row.approved_scope_hash === undefined ? {} : { approved_scope_hash: row.approved_scope_hash as string | null }), ...(row.workflow_id === undefined ? {} : { workflow_id: row.workflow_id as string | null }) };
}
function optionalText(value: unknown): boolean { return value === undefined || value === null || text(value); }
function optionalTextArray(value: unknown, max: number): boolean { return value === undefined || (Array.isArray(value) && value.length <= max && value.every(text)); }
function optionalChecks(value: unknown): boolean { return value === undefined || (Array.isArray(value) && value.length <= 32 && value.every((item) => { const row = record(item); return !!row && text(row.id) && text(row.state) && optionalText(row.detail); })); }
function optionalAttempts(value: unknown): boolean { return value === undefined || (Array.isArray(value) && value.length <= 3 && value.every((item) => { const row = record(item); return !!row && text(row.id) && integer(row.number) && row.number > 0 && text(row.state) && optionalText(row.detail); })); }
