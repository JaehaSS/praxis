import { describe, expect, it } from "vitest";
import { mermaidLabel, parseWorkflowPlanJson, validateWorkflowPlan, workflowMermaid } from "./graph";
import type { WorkflowSpec } from "./types";

const plan: WorkflowSpec = {
  schema_version: 1, project_ref: "repo", base_commit: "0000000000000000000000000000000000000000", execution_profile_id: "default", final_task_id: "b",
  phases: [{ id: "p", name: "Phase", order: 1 }],
  tasks: [
    { id: "a", phase_id: "p", kind: "agent", objective: "first", write_paths: [], resource_requests_by_step: {}, input_artifacts: [], output_contract: { include_paths: ["src"], exclude_paths: [] }, checks: [], manual_acceptance: [], retry_policy: { max_attempts: 1, auto_retry_transient: false } },
    { id: "b", phase_id: "p", kind: "integration", objective: "last", write_paths: [], resource_requests_by_step: {}, input_artifacts: [], output_contract: { include_paths: ["src"], exclude_paths: [] }, checks: [{ id: "check", profile_id: "default" }], manual_acceptance: [], retry_policy: { max_attempts: 1, auto_retry_transient: false } },
  ], edges: [{ from: "a", to: "b" }], limits: { max_tasks: 10, max_edges: 10, max_attempts_per_task: 3 },
};

describe("workflow plan graph", () => {
  it("accepts the zero-edge final-only plan and enforces declared graph limits", () => {
    expect(validateWorkflowPlan({ ...plan, tasks: [plan.tasks[1]], edges: [], limits: { ...plan.limits, max_edges: 0 } }).spec).not.toBeNull();
    expect(validateWorkflowPlan({ ...plan, limits: { ...plan.limits, max_tasks: 1 } }).spec).toBeNull();
    expect(validateWorkflowPlan({ ...plan, limits: { ...plan.limits, max_edges: 0 } }).spec).toBeNull();
    const cyclic: Record<string, unknown> = { ...plan }; cyclic.self = cyclic;
    expect(validateWorkflowPlan(cyclic).spec).toBeNull();
  });
  it("rejects cycles before a Runner request", () => {
    expect(validateWorkflowPlan({ ...plan, edges: [{ from: "a", to: "b" }, { from: "b", to: "a" }] }).issues.map((issue) => issue.message)).toContain("순환 의존성은 허용되지 않습니다.");
  });

  it("never allows hostile task text to become Mermaid syntax", () => {
    const hostile = { ...plan, tasks: [{ ...plan.tasks[0], objective: "<script> \"[]" }, plan.tasks[1]] };
    const checked = validateWorkflowPlan(hostile);
    expect(checked.spec).not.toBeNull();
    const chart = workflowMermaid(checked.spec!);
    expect(chart).not.toContain("<script>");
    expect(chart).not.toContain('"[]');
    expect(mermaidLabel('&"[]<>')).toBe("&#x26;&#x22;&#x5b;&#x5d;&#x3c;&#x3e;");
  });

  it("rejects malformed nested task fields before detail rendering", () => {
    const result = validateWorkflowPlan({ ...plan, tasks: [{ ...plan.tasks[0], input_artifacts: [null] }, plan.tasks[1]] });
    expect(result.spec).toBeNull();
    expect(result.issues.some((issue) => issue.path.endsWith("input_artifacts"))).toBe(true);
  });

  it("matches resource-step and byte-size admission limits without throwing on unusual values", () => {
    const invalidResource = validateWorkflowPlan({ ...plan, tasks: [{ ...plan.tasks[0], resource_requests_by_step: { nope: [], execute: [{ resource_id: "db", mode: "shared_read", units: 2 }] } }, plan.tasks[1]] });
    expect(invalidResource.spec).toBeNull();
    expect(invalidResource.issues.some((issue) => issue.path.endsWith("resource_requests_by_step"))).toBe(true);
    expect(validateWorkflowPlan(BigInt(1)).spec).toBeNull();
    expect(parseWorkflowPlanJson(`{"x":"${"한".repeat(90_000)}"}`).spec).toBeNull();
  });
});
