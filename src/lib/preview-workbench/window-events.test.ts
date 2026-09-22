import { describe, expect, it, vi } from "vitest";
import { isPreviewToolbarEntry, matchesToolbarIdentity, publishToolbarMessage, relayToolbarMessage, toolbarHeight } from "./window-events";
import type { PreviewWorkbenchState } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const state = (patch: Partial<PreviewWorkbenchState> = {}): PreviewWorkbenchState => ({
  key: "local:1", taskId: 1, appEpoch: "epoch", busy: "idle", url: "http://localhost:3000", convoActive: false,
  takenOver: false, supported: true, unsupportedReason: null, draft: "", displayUrl: null, pending: null,
  inFlight: null, error: null, lastAction: null, revision: 1, ...patch,
});
const identity = { appEpoch: "epoch", taskId: 1, toolbarLabel: "previewbar-1-1", windowGeneration: 1 };

describe("preview toolbar events", () => {
  it("recognizes only the toolbar entry", () => {
    expect(isPreviewToolbarEntry("?window=preview-toolbar&task=1")).toBe(true);
    expect(isPreviewToolbarEntry("?window=preview-toolbar-old")).toBe(false);
  });

  it("uses the compact toolbar height range", () => {
    expect(toolbarHeight(state())).toBe(96);
    expect(toolbarHeight(state({ busy: "busy" }))).toBe(128);
    expect(toolbarHeight(state({ error: "failed" }))).toBe(160);
  });

  it("rejects stale toolbar identity", () => {
    expect(matchesToolbarIdentity({ ...identity, kind: "ready", correlationId: "r" }, identity)).toBe(true);
    expect(matchesToolbarIdentity({ ...identity, kind: "ready", appEpoch: "old", correlationId: "r" }, identity)).toBe(false);
  });

  it("uses only the restricted relay and publish commands", async () => {
    await relayToolbarMessage({ ...identity, kind: "ready", correlationId: "r" });
    await publishToolbarMessage({ ...identity, kind: "ack", correlationId: "r", status: "queued" });
    expect(invoke).toHaveBeenNthCalledWith(1, "plugin:preview-workbench|relay", expect.any(Object));
    expect(invoke).toHaveBeenNthCalledWith(2, "plugin:preview-workbench|publish", expect.any(Object));
  });
});
