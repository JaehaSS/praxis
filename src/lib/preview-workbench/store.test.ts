import { describe, expect, it } from "vitest";
import { PreviewWorkbenchStore, previewPending } from "./store";

const remote = (patch = {}) => ({
  taskId: 1,
  appEpoch: "epoch",
  busy: "idle" as const,
  url: "http://localhost:3000",
  convoActive: false,
  takenOver: false,
  supported: true,
  unsupportedReason: null,
  ...patch,
});

describe("PreviewWorkbenchStore", () => {
  it("keeps task keys distinct across hosts", () => {
    const store = new PreviewWorkbenchStore();
    store.setDraft("local:1", 1, "A");
    store.setDraft("remote:1", 1, "B");
    expect(store.get("local:1", 1).draft).toBe("A");
    expect(store.get("remote:1", 1).draft).toBe("B");
  });

  it("does not flush unknown work but permits a turn while taken over", () => {
    const store = new PreviewWorkbenchStore();
    store.queue("local:1", 1, previewPending("ask", "a", "manual"));
    expect(store.claim("local:1", 1)).toBeNull();
    store.sync("local:1", remote({ takenOver: true }));
    expect(store.claim("local:1", 1)?.correlationId).toBe("a");
  });

  it("claims before awaiting and ignores an older failure after replacement", () => {
    const store = new PreviewWorkbenchStore();
    store.sync("local:1", remote());
    store.queue("local:1", 1, previewPending("old", "old", "manual"));
    expect(store.claim("local:1", 1)?.correlationId).toBe("old");
    store.queue("local:1", 1, previewPending("new", "new", "manual"));
    store.fail("local:1", 1, "old", "send failed");
    expect(store.get("local:1", 1).pending?.message).toBe("new");
    expect(store.get("local:1", 1).error).toBe("send failed");
  });

  it("should_invalidate_an_old_flight_when_the_app_epoch_changes", () => {
    const store = new PreviewWorkbenchStore();
    store.sync("local:1", remote());
    store.queue("local:1", 1, previewPending("old", "old", "manual"));
    store.claim("local:1", 1);
    store.bindRequest("local:1", 1, "old", "epoch:1:1");
    store.sync("local:1", remote({ appEpoch: "new-epoch" }));

    expect(store.get("local:1", 1)).toMatchObject({ pending: null, inFlight: null });
  });

  it("removes a task so stale work cannot be reused", () => {
    const store = new PreviewWorkbenchStore();
    store.sync("local:1", remote());
    store.remove("local:1");
    expect(store.all()).toEqual([]);
  });

  it("binds a correlation before refresh and clears it with its deleted task", () => {
    const store = new PreviewWorkbenchStore();
    expect(store.reserveCorrelation("local:1", "ui-1", "질문")).toBe(true);
    expect(store.reserveCorrelation("local:1", "ui-1", "질문")).toBe(false);
    store.remove("local:1");
    expect(store.reserveCorrelation("local:1", "ui-1", "새 질문")).toBe(true);
  });

  it("drops terminal work without replacing a newer pending question", () => {
    const store = new PreviewWorkbenchStore();
    store.sync("local:1", remote());
    store.queue("local:1", 1, previewPending("old", "old", "manual"));
    store.claim("local:1", 1);
    store.queue("local:1", 1, previewPending("new", "new", "manual"));
    store.terminal("local:1", 1, "old", "rejected");
    expect(store.get("local:1", 1)).toMatchObject({ pending: { correlationId: "new" }, inFlight: null, error: "rejected" });
  });

  it("disposes terminal state without affecting temporary unsupported state", () => {
    const store = new PreviewWorkbenchStore();
    store.sync("local:1", remote());
    store.setDraft("local:1", 1, "draft");
    store.queue("local:1", 1, previewPending("ask", "ask", "manual"));
    store.claim("local:1", 1);
    store.unsupported("local:1", 1, "temporary");
    expect(store.get("local:1", 1).draft).toBe("draft");
    store.dispose("local:1", 1, "terminal");
    expect(store.get("local:1", 1)).toMatchObject({ draft: "", pending: null, inFlight: null, unsupportedReason: "terminal" });
  });

  it("retains a bound receipt outcome after its flight resolves", () => {
    const store = new PreviewWorkbenchStore();
    store.sync("local:1", remote());
    store.queue("local:1", 1, previewPending("ask", "ui-1", "manual"));
    store.claim("local:1", 1);
    store.bindRequest("local:1", 1, "ui-1", "request-1");
    store.acknowledge("local:1", 1, "ui-1", "accepted", "request-1");
    store.resolve("local:1", 1, "ui-1");
    expect(store.get("local:1", 1).receipt).toEqual({ correlationId: "ui-1", status: "accepted", requestId: "request-1" });
  });

  it("retains a queued outcome after later pending work changes", () => {
    const store = new PreviewWorkbenchStore();
    store.acknowledge("local:1", 1, "queued", "queued");
    store.queue("local:1", 1, previewPending("new", "new", "manual"));
    store.cancel("local:1", 1, "new");
    expect(store.outcome("local:1", "queued")).toEqual({ correlationId: "queued", status: "queued" });
  });
});
