// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PreviewToolbarWindow } from "./PreviewToolbarWindow";
import { PREVIEW_TOOLBAR_EVENT, type ToolbarAck, type ToolbarError, type ToolbarState } from "../../lib/preview-workbench/window-events";
import type { PreviewWorkbenchState } from "../../lib/preview-workbench/types";

const mocks = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => { mocks.handlers.set(name, handler); return () => mocks.handlers.delete(name); }),
  adoptTheme: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("../../lib/themes", () => ({ adoptTheme: mocks.adoptTheme }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const state = (patch: Partial<PreviewWorkbenchState> = {}): PreviewWorkbenchState => ({
  key: "local:1", taskId: 1, appEpoch: "epoch", busy: "idle", url: "http://localhost:3000", convoActive: false,
  takenOver: false, supported: true, unsupportedReason: null, draft: "질문", displayUrl: null, pending: null,
  inFlight: null, error: null, lastAction: null, revision: 1, ...patch,
});
const message = (revision = 1, patch: Partial<PreviewWorkbenchState> = {}): ToolbarState => ({
  kind: "state", appEpoch: "epoch", taskId: 1, toolbarLabel: "previewbar-1-1", windowGeneration: 1,
  correlationId: "ready", revision, state: state(patch), theme: {} as ToolbarState["theme"],
});

let root: Root | null = null;
let host: HTMLDivElement | null = null;
async function publish(payload: ToolbarState) {
  await act(async () => mocks.handlers.get(PREVIEW_TOOLBAR_EVENT)?.({ payload }));
}

async function setDraft(value: string) {
  const input = host!.querySelector("textarea") as HTMLTextAreaElement;
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  setter?.call(input, value);
  await act(async () => input.dispatchEvent(new Event("input", { bubbles: true })));
}

beforeEach(async () => {
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  await act(async () => root?.render(<PreviewToolbarWindow />));
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove(); root = null; host = null; vi.clearAllMocks();
});

describe("PreviewToolbarWindow", () => {
  it("retries ready until a verified snapshot arrives", async () => {
    expect(mocks.invoke).toHaveBeenCalledWith("plugin:preview-workbench|relay", expect.objectContaining({ message: expect.objectContaining({ kind: "ready" }) }));
    await publish(message());
    expect(host?.textContent).toContain("localhost:3000");
    expect(mocks.adoptTheme).toHaveBeenCalledOnce();
  });

  it("relays strip actions for its fixed task", async () => {
    await publish(message());
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    expect(mocks.invoke).toHaveBeenLastCalledWith("plugin:preview-workbench|relay", expect.objectContaining({ message: expect.objectContaining({ kind: "intent", action: "ask", taskId: 1, text: "질문" }) }));
  });

  it("ignores stale revision, epoch, and generation state", async () => {
    await publish(message(2, { draft: "new" }));
    await publish(message(1, { draft: "old" }));
    await publish({ ...message(3, { draft: "old epoch" }), appEpoch: "old" });
    await publish({ ...message(3, { draft: "old generation" }), windowGeneration: 0 });
    expect(host?.querySelector("textarea")?.getAttribute("value")).toBeNull();
    expect((host?.querySelector("textarea") as HTMLTextAreaElement).value).toBe("new");
  });

  it("keeps a newer local draft until its correlated snapshot arrives", async () => {
    await publish(message());
    await setDraft("빠른 입력");
    const draftCalls = mocks.invoke.mock.calls as unknown as Array<[string, { message: { correlationId: string } }]>;
    const draftCall = draftCalls[draftCalls.length - 1][1].message;
    await publish(message(2, { draft: "이전 에코" }));
    expect((host?.querySelector("textarea") as HTMLTextAreaElement).value).toBe("빠른 입력");
    await publish({ ...message(3, { draft: "빠른 입력" }), correlationId: draftCall.correlationId });
    expect((host?.querySelector("textarea") as HTMLTextAreaElement).value).toBe("빠른 입력");
  });

  it("drops its local draft buffer after an accepted host snapshot", async () => {
    await publish(message());
    await setDraft("보낼 질문");
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    const calls = mocks.invoke.mock.calls as unknown as Array<[string, { message: { correlationId: string } }]>;
    const correlationId = calls[calls.length - 1][1].message.correlationId;
    await publish({ ...message(2, { draft: "", receipt: { correlationId, status: "accepted", requestId: "request-1" } }), correlationId });
    expect((host?.querySelector("textarea") as HTMLTextAreaElement).value).toBe("");
  });

  it("keeps a rapid local submission visible when the host does not respond", async () => {
    vi.useFakeTimers();
    await publish(message(1, { draft: "" }));
    await setDraft("빠른 제출");
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    await act(async () => vi.advanceTimersByTime(3_000));
    expect((host?.querySelector("textarea") as HTMLTextAreaElement).value).toBe("빠른 제출");
    expect(host?.textContent).toContain("응답 시간이 초과");
    vi.useRealTimers();
  });

  it("makes room for a connection error and shrinks after a fresh snapshot", async () => {
    vi.useFakeTimers();
    await publish(message());
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    await act(async () => vi.advanceTimersByTime(3_000));
    expect(mocks.invoke).toHaveBeenLastCalledWith("plugin:preview-workbench|relay", expect.objectContaining({
      message: expect.objectContaining({ action: "resize", height: 160 }),
    }));
    await publish(message(2));
    expect(host?.textContent).not.toContain("응답 시간이 초과");
    expect(mocks.invoke).toHaveBeenLastCalledWith("plugin:preview-workbench|relay", expect.objectContaining({
      message: expect.objectContaining({ action: "resize", height: 96 }),
    }));
    vi.useRealTimers();
  });

  it("clears an acknowledged submitted draft but keeps a newer edit", async () => {
    await publish(message(1, { draft: "" }));
    await setDraft("첫 제출");
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    const calls = mocks.invoke.mock.calls as unknown as Array<[string, { message: { correlationId: string } }]>;
    const first = calls[calls.length - 1][1].message.correlationId;
    const identity = { appEpoch: "epoch", taskId: 1, toolbarLabel: "previewbar-1-1", windowGeneration: 1, correlationId: first };
    await act(async () => mocks.handlers.get(PREVIEW_TOOLBAR_EVENT)?.({ payload: { ...identity, kind: "ack", status: "accepted", requestId: "request-1" } satisfies ToolbarAck }));
    expect((host?.querySelector("textarea") as HTMLTextAreaElement).value).toBe("");

    await setDraft("두번째 제출");
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    const second = calls[calls.length - 1][1].message.correlationId;
    await setDraft("새 초안");
    await act(async () => mocks.handlers.get(PREVIEW_TOOLBAR_EVENT)?.({ payload: { ...identity, correlationId: second, kind: "ack", status: "accepted", requestId: "request-2" } satisfies ToolbarAck }));
    expect((host?.querySelector("textarea") as HTMLTextAreaElement).value).toBe("새 초안");
  });

  it("times out only without a host acknowledgement and retries the same intent", async () => {
    vi.useFakeTimers();
    await publish(message());
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    const calls = mocks.invoke.mock.calls as unknown as Array<[string, { message: object }]>;
    const first = calls[calls.length - 1][1].message;
    await act(async () => vi.advanceTimersByTime(3_000));
    expect(host?.textContent).toContain("응답 시간이 초과");
    await setDraft("나중 초안");
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "재시도")?.click());
    expect(calls[calls.length - 1][1].message).toMatchObject(first);
    vi.useRealTimers();
  });

  it("settles an acknowledgement received before relay invocation resolves", async () => {
    vi.useFakeTimers();
    let resolve!: () => void;
    const delayed = new Promise<void>((done) => { resolve = done; });
    await publish(message());
    mocks.invoke.mockImplementationOnce((() => delayed) as never);
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    const calls = mocks.invoke.mock.calls as unknown as Array<[string, { message: { correlationId: string } }]>;
    const intent = calls[calls.length - 1][1].message;
    const ack: ToolbarAck = { kind: "ack", appEpoch: "epoch", taskId: 1, toolbarLabel: "previewbar-1-1", windowGeneration: 1, correlationId: intent.correlationId, status: "queued" };
    await act(async () => mocks.handlers.get(PREVIEW_TOOLBAR_EVENT)?.({ payload: ack }));
    resolve();
    await act(async () => vi.advanceTimersByTime(3_000));
    expect(host?.textContent).not.toContain("응답 시간이 초과");
    vi.useRealTimers();
  });

  it("settles a control request from its current state despite an older receipt", async () => {
    vi.useFakeTimers();
    await publish(message(1, { receipt: { correlationId: "old", status: "accepted", requestId: "old-request" } }));
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "직접 제어")?.click());
    const calls = mocks.invoke.mock.calls as unknown as Array<[string, { message: { correlationId: string } }]>;
    const intent = calls[calls.length - 1][1].message;
    await publish({ ...message(2, { receipt: { correlationId: "old", status: "accepted", requestId: "old-request" } }), correlationId: intent.correlationId });
    await act(async () => vi.advanceTimersByTime(3_000));
    expect(host?.textContent).not.toContain("응답 시간이 초과");
    vi.useRealTimers();
  });

  it("retains an accepted request ID when a correlated host error needs retry", async () => {
    await publish(message());
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    const intentCalls = mocks.invoke.mock.calls as unknown as Array<[string, { message: { correlationId: string } }]>;
    const intent = intentCalls[intentCalls.length - 1][1].message;
    const identity = { appEpoch: "epoch", taskId: 1, toolbarLabel: "previewbar-1-1", windowGeneration: 1, correlationId: intent.correlationId };
    const ack: ToolbarAck = { ...identity, kind: "ack", status: "accepted", requestId: "request-1" };
    const error: ToolbarError = { ...identity, kind: "error", requestId: "request-1", error: "host failed" };
    await act(async () => mocks.handlers.get(PREVIEW_TOOLBAR_EVENT)?.({ payload: ack }));
    await act(async () => mocks.handlers.get(PREVIEW_TOOLBAR_EVENT)?.({ payload: error }));
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "재시도")?.click());
    const retryCalls = mocks.invoke.mock.calls as unknown as Array<[string, { message: object }]>;
    expect(retryCalls[retryCalls.length - 1][1].message).toMatchObject({ ...intent, requestId: "request-1" });
  });

  it("uses a fresh correlation after the toolbar reopens", async () => {
    await publish(message());
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    const firstCalls = mocks.invoke.mock.calls as unknown as Array<[string, { message: { correlationId: string; action?: string } }]>;
    const first = firstCalls[firstCalls.length - 1][1].message.correlationId;
    await act(async () => root?.unmount());
    root = createRoot(host!);
    await act(async () => root?.render(<PreviewToolbarWindow />));
    await publish(message());
    await act(async () => [...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.click());
    const reopenedCalls = mocks.invoke.mock.calls as unknown as Array<[string, { message: { correlationId: string } }]>;
    expect(reopenedCalls[reopenedCalls.length - 1][1].message.correlationId).not.toBe(first);
  });

  it("removes the toolbar listener when closed", async () => {
    await act(async () => root?.unmount());
    expect(mocks.handlers.has(PREVIEW_TOOLBAR_EVENT)).toBe(false);
  });
});
