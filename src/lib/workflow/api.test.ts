import fixture from "../../../src-tauri/tests/fixtures/workflow-spec-v1.json";
import { describe, expect, it, vi } from "vitest";
import { createWorkflowTransport } from "./api";
import { reconcileWorkflowEvents } from "./state";

describe("workflow transport", () => {
  it("does not confuse task limits with the number of saved workflow runs", async () => {
    const runs = Array.from({ length: 101 }, (_, i) => ({ id: `run-${i}`, revision: 1, state: "draft", last_sequence: 0, spec: fixture, nodes: [] }));
    const api = createWorkflowTransport({ request: vi.fn(async () => runs) as never });
    await expect(api.capability()).resolves.toMatchObject({ supported: true });
    await expect(api.list()).resolves.toHaveLength(101);
  });
  it("rejects oversized remote specs before they reach graph rendering", async () => {
    const spec = structuredClone(fixture);
    spec.tasks[0].objective = "한".repeat(100_000);
    const snapshot = { id: "run", revision: 1, state: "draft", last_sequence: 0, spec, nodes: [] };
    const api = createWorkflowTransport({ request: vi.fn(async () => [snapshot]) as never });
    await expect(api.capability()).resolves.toMatchObject({ supported: false });
    await expect(api.list()).rejects.toThrow("계획 spec");
  });
  it("treats an older Runner list endpoint as unsupported", async () => {
    const request = vi.fn(async () => { throw new Error("Runner 요청 실패 (404): missing"); });
    await expect(createWorkflowTransport({ request: request as never }).capability()).resolves.toEqual({ supported: false, reason: "연결된 Runner가 Workflow를 아직 지원하지 않습니다." });
  });

  it("sends action request IDs and expected revisions, then leaves completion to the server", async () => {
    const request = vi.fn(async () => ({ request_id: "r-1", revision: 4, accepted: true }));
    const api = createWorkflowTransport({ request: request as never });
    await expect(api.action("run/id", "pause", 4, "r-1")).resolves.toMatchObject({ accepted: true });
    expect(request).toHaveBeenCalledWith("/v1/workflows/run%2Fid/pause", { method: "POST", body: JSON.stringify({ request_id: "r-1", expected_revision: 4 }) });
    const snapshot = { id: "run", revision: 4, state: "running" as const, last_sequence: 8, spec: {} as never, nodes: [] };
    expect(reconcileWorkflowEvents(snapshot, [{ sequence: 7, kind: "old" }, { sequence: 9, kind: "accepted" }])).toEqual({ snapshot, cursor: 9 });
  });

  it("uses the validation endpoint before a start request can carry an approved scope hash", async () => {
    const request = vi.fn(async () => ({ request_id: "v-1", revision: 4, accepted: true, approved_scope_hash: "scope" }));
    const api = createWorkflowTransport({ request: request as never });
    await api.validate("run", 4, "v-1");
    await api.action("run", "start", 4, "s-1", "scope");
    expect(request.mock.calls[0]).toEqual(["/v1/workflows/run/validate", { method: "POST", body: JSON.stringify({ request_id: "v-1", expected_revision: 4 }) }]);
    expect(request.mock.calls[1]).toEqual(["/v1/workflows/run/start", { method: "POST", body: JSON.stringify({ request_id: "s-1", expected_revision: 4, approved_scope_hash: "scope" }) }]);
  });

  it("does not mistake malformed successful JSON for workflow support", async () => {
    const api = createWorkflowTransport({ request: vi.fn(async () => ({})) as never });
    await expect(api.capability()).resolves.toEqual({ supported: false, reason: "Workflow Runner를 확인할 수 없습니다." });
    await expect(api.list()).rejects.toThrow("Workflow 응답 형식 오류");
  });
});
