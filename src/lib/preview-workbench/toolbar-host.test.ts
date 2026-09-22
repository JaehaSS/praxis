import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PreviewWorkbenchHost } from "../../components/use-preview-workbench-host";
import type { PreviewWorkbenchState } from "./types";
import { ToolbarHost } from "./toolbar-host";

const mocks = vi.hoisted(() => ({ publish: vi.fn(async (_message: unknown): Promise<void> => undefined), theme: vi.fn(() => ({ id: "dark" })) }));
vi.mock("../themes", () => ({ getActiveTheme: mocks.theme }));
vi.mock("./window-events", async (importOriginal) => ({ ...await importOriginal<typeof import("./window-events")>(), publishToolbarMessage: mocks.publish }));

const state = (patch: Partial<PreviewWorkbenchState> = {}): PreviewWorkbenchState => ({
  key: "local:1", taskId: 1, appEpoch: "epoch", busy: "idle", url: "http://localhost:3000", convoActive: false,
  takenOver: false, supported: true, unsupportedReason: null, draft: "", displayUrl: null, pending: null,
  inFlight: null, error: null, lastAction: null, revision: 1, ...patch,
});
const ready = { kind: "ready" as const, appEpoch: "epoch", taskId: 1, toolbarLabel: "bar", windowGeneration: 1, correlationId: "ready" };

function host(stateFor = vi.fn(() => state())) {
  return {
    refresh: vi.fn(async () => undefined), stateFor, submit: vi.fn(async () => undefined), cancelPending: vi.fn(),
    setDraft: vi.fn(), takeOver: vi.fn(async () => undefined), release: vi.fn(async () => undefined), receiptFor: vi.fn(() => null),
  } as unknown as PreviewWorkbenchHost;
}

describe("ToolbarHost", () => {
  beforeEach(() => {
    mocks.publish.mockReset().mockResolvedValue(undefined);
    mocks.theme.mockReset().mockReturnValue({ id: "dark" });
  });
  it("retries an unchanged snapshot after state delivery fails", async () => {
    const adapter = new ToolbarHost({ current: host() });
    await adapter.handle(ready);
    mocks.publish.mockRejectedValueOnce(new Error("state lost"));
    await expect(adapter.publish()).resolves.toBeUndefined();
    await adapter.publish();
    expect(mocks.publish.mock.calls.filter(([message]) => (message as { kind: string }).kind === "state")).toHaveLength(2);
    await adapter.publish();
    expect(mocks.publish.mock.calls.filter(([message]) => (message as { kind: string }).kind === "state")).toHaveLength(2);
    adapter.clear();
  });

  it("catches a lost acknowledgement and retries without a state change", async () => {
    const adapter = new ToolbarHost({ current: host(vi.fn(() => state({ receipt: { correlationId: "ask", status: "accepted", requestId: "r-1" } }))) });
    await adapter.handle(ready);
    mocks.publish.mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error("ack lost"));
    await expect(adapter.publish()).resolves.toBeUndefined();
    await adapter.publish();
    expect(mocks.publish.mock.calls.filter(([message]) => (message as { kind: string }).kind === "ack")).toHaveLength(2);
    adapter.clear();
  });

  it("coalesces changes arriving while an older snapshot is in flight", async () => {
    let resolve!: () => void;
    const delayed = new Promise<void>((done) => { resolve = done; });
    const current = host();
    const adapter = new ToolbarHost({ current });
    await adapter.handle(ready);
    mocks.publish.mockImplementationOnce(() => delayed);
    const sending = adapter.publish();
    (current.stateFor as ReturnType<typeof vi.fn>).mockReturnValue(state({ revision: 2, draft: "new" }));
    await adapter.publish();
    expect(mocks.publish).toHaveBeenCalledTimes(1);
    resolve();
    await sending;
    expect(mocks.publish).toHaveBeenLastCalledWith(expect.objectContaining({ state: expect.objectContaining({ draft: "new" }) }));
    adapter.clear();
  });

  it("does not acknowledge a session closed while state delivery was pending", async () => {
    let resolve!: () => void;
    const delayed = new Promise<void>((done) => { resolve = done; });
    const adapter = new ToolbarHost({ current: host(vi.fn(() => state({ receipt: { correlationId: "ask", status: "accepted", requestId: "r-1" } }))) });
    await adapter.handle(ready);
    mocks.publish.mockImplementationOnce(() => delayed);
    const sending = adapter.publish();
    adapter.closeTask(1);
    resolve();
    await sending;
    expect(mocks.publish).toHaveBeenCalledTimes(1);
    await adapter.publish();
    expect(mocks.publish).toHaveBeenCalledTimes(1);
    adapter.clear();
  });

  it("publishes only changed snapshots and keeps unsupported state visible", async () => {
    const current = host();
    const adapter = new ToolbarHost({ current });
    await adapter.handle(ready);
    await adapter.publish();
    await adapter.publish();
    expect(mocks.publish).toHaveBeenCalledTimes(1);

    (current.stateFor as ReturnType<typeof vi.fn>).mockReturnValue(state({ revision: 2, supported: false, unsupportedReason: "완료됨" }));
    await adapter.publish();
    await adapter.publish();
    expect(mocks.publish).toHaveBeenCalledTimes(2);
    adapter.closeTask(1);
    await adapter.publish();
    expect(mocks.publish).toHaveBeenCalledTimes(2);
  });

  it("submits repeated correlations so the host can reject a changed body", async () => {
    const current = host();
    const adapter = new ToolbarHost({ current });
    await adapter.handle(ready);
    await adapter.handle({ ...ready, kind: "intent", action: "ask", correlationId: "same", text: "첫 질문" });
    await adapter.handle({ ...ready, kind: "intent", action: "ask", correlationId: "same", text: "바뀐 질문" });
    expect(current.submit).toHaveBeenNthCalledWith(1, "local:1", 1, "첫 질문", "same");
    expect(current.submit).toHaveBeenNthCalledWith(2, "local:1", 1, "바뀐 질문", "same");
  });

  it("rejects an unbound retry request ID without submitting it", async () => {
    const current = host();
    const adapter = new ToolbarHost({ current });
    await adapter.handle(ready);
    await adapter.handle({ ...ready, kind: "intent", action: "ask", correlationId: "unknown", requestId: "request-1", text: "질문" });
    expect(current.submit).not.toHaveBeenCalled();
    expect(mocks.publish).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "error", correlationId: "unknown", requestId: "request-1" }));
  });

  it("passes a matching retry ID through the host's safe recovery path", async () => {
    const current = host(vi.fn(() => state({ pending: { correlationId: "same", message: "질문", source: "manual" }, inFlight: { correlationId: "same", message: "질문", source: "manual", requestId: "request-1" } })));
    (current.receiptFor as ReturnType<typeof vi.fn>).mockReturnValue({ correlationId: "same", status: "accepted", requestId: "request-1" });
    const adapter = new ToolbarHost({ current });
    await adapter.handle(ready);
    await adapter.handle({ ...ready, kind: "intent", action: "ask", correlationId: "same", requestId: "request-1", text: "질문" });
    expect(current.submit).toHaveBeenCalledWith("local:1", 1, "질문", "same");
  });

  it("keeps the last non-resize correlation on published state", async () => {
    const current = host();
    const adapter = new ToolbarHost({ current });
    await adapter.handle(ready);
    await adapter.handle({ ...ready, kind: "intent", action: "take_over", correlationId: "control" });
    await adapter.handle({ ...ready, kind: "intent", action: "resize", correlationId: "resize", height: 96 });
    await adapter.publish();
    expect(mocks.publish).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "state", correlationId: "control" }));
  });

  it("returns scoped relay errors instead of leaking rejected control promises", async () => {
    const current = host();
    (current.takeOver as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("takeover failed"));
    const adapter = new ToolbarHost({ current });
    await adapter.handle(ready);
    await adapter.handle({ ...ready, kind: "intent", action: "take_over", correlationId: "take" });
    expect(mocks.publish).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "error", correlationId: "take", error: "Error: takeover failed" }));
  });
});
