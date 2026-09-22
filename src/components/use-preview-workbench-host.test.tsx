// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePreviewWorkbenchHost, type PreviewWorkbenchHost, type PreviewWorkbenchTask } from "./use-preview-workbench-host";

const mocks = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => { mocks.handlers.set(name, handler); return () => {}; }),
  prepare: vi.fn(async () => ({ requestId: "r-1", status: "prepared", accepted: false, running: false, retryable: true })),
  receipt: vi.fn(async () => ({ requestId: "r-1", status: "prepared", accepted: false, running: false, retryable: true })),
  release: vi.fn(async () => undefined),
  send: vi.fn(async () => ({ requestId: "r-1", status: "accepted", accepted: true, running: true, retryable: false })),
  state: vi.fn(),
  takeover: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("../lib/ipc", () => ({
  previewRelease: mocks.release,
  previewTakeOver: mocks.takeover,
  previewWorkbenchPrepare: mocks.prepare,
  previewWorkbenchReceipt: mocks.receipt,
  previewWorkbenchSend: mocks.send,
  previewWorkbenchState: mocks.state,
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const idle = (taskId = 1) => ({ taskId, appEpoch: "epoch", busy: "idle" as const, url: "http://localhost:3000", convoActive: false, takenOver: false, supported: true, unsupportedReason: null });
const busy = (taskId = 1) => ({ ...idle(taskId), busy: "busy" as const });
const tasks: PreviewWorkbenchTask[] = [
  { key: "local:1", taskId: 1, supported: true, unsupportedReason: null, terminal: false },
  { key: "local:2", taskId: 2, supported: true, unsupportedReason: null, terminal: false },
];

let host: PreviewWorkbenchHost | null = null;
let root: Root | null = null;
let element: HTMLDivElement | null = null;
let acceptedHandler = vi.fn();
let currentTasks = tasks;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function Harness() {
  host = usePreviewWorkbenchHost({ tasks: currentTasks, onAccepted: acceptedHandler });
  return null;
}

async function renderTasks(nextTasks: PreviewWorkbenchTask[]) {
  currentTasks = nextTasks;
  await act(async () => root?.render(<Harness />));
}

beforeEach(async () => {
  mocks.handlers.clear();
  acceptedHandler = vi.fn();
  currentTasks = tasks;
  mocks.state.mockResolvedValue(idle());
  element = document.createElement("div"); document.body.append(element); root = createRoot(element);
  await act(async () => root?.render(<Harness />));
});

afterEach(async () => {
  await act(async () => root?.unmount());
  element?.remove(); host = null; root = null; element = null;
  vi.clearAllMocks();
});

describe("usePreviewWorkbenchHost", () => {
  it.each(["idle", "busy"] as const)("keeps the latest submission when %s state queries resolve out of order", async (status) => {
    const older = deferred<ReturnType<typeof idle>>();
    mocks.state.mockImplementationOnce(() => older.promise).mockResolvedValueOnce({ ...idle(), busy: status });
    const first = host!.submit("local:1", 1, "이전 질문", "older");
    await act(async () => host?.submit("local:1", 1, "최신 질문", "newer"));
    await act(async () => { older.resolve(idle()); await first; });
    if (status === "idle") {
      expect(mocks.send).toHaveBeenCalledOnce();
      expect(mocks.send).toHaveBeenCalledWith(1, "r-1", expect.stringContaining("최신 질문"), "http://localhost:3000", "manual");
    } else {
      expect(mocks.send).not.toHaveBeenCalled();
      expect(host?.stateFor("local:1", 1).pending?.message).toBe("최신 질문");
    }
  });

  it("recovers a lost prepare reply with the same correlation and bound URL", async () => {
    mocks.prepare.mockRejectedValueOnce(new Error("prepare reply lost"));
    await act(async () => host?.submit("local:1", 1, "폼 확인", "ui-lost"));
    expect(host?.stateFor("local:1", 1).inFlight?.correlationId).toBe("ui-lost");
    mocks.state.mockResolvedValue({ ...idle(), url: "http://localhost:3000/next" });
    await act(async () => host?.submit("local:1", 1, "폼 확인", "ui-lost"));
    expect(mocks.prepare).toHaveBeenNthCalledWith(2, 1, "ui-lost", expect.any(String), "http://localhost:3000", "manual");
    expect(mocks.send).toHaveBeenCalledOnce();
    expect(mocks.send).toHaveBeenCalledWith(1, "r-1", expect.any(String), "http://localhost:3000", "manual");
  });

  it("restores the question when a recovered prepare receipt was rejected", async () => {
    mocks.prepare.mockRejectedValueOnce(new Error("prepare reply lost"));
    await act(async () => host?.submit("local:1", 1, "폼 확인", "ui-lost"));
    mocks.prepare.mockResolvedValueOnce({ requestId: "r-1", status: "rejected", accepted: false, running: false, retryable: false });
    await act(async () => host?.submit("local:1", 1, "폼 확인", "ui-lost"));
    expect(mocks.send).not.toHaveBeenCalled();
    expect(host?.stateFor("local:1", 1)).toMatchObject({ draft: "폼 확인", pending: null, inFlight: null });
    await act(async () => host?.refresh("local:1", 1));
    expect(mocks.prepare).toHaveBeenCalledTimes(2);
  });

  it("sends one accepted question without touching another task", async () => {
    await act(async () => host?.submit("local:1", 1, "폼 확인"));
    expect(mocks.prepare).toHaveBeenCalledOnce();
    expect(mocks.send).toHaveBeenCalledWith(1, "r-1", expect.stringContaining("폼 확인"), "http://localhost:3000", "manual");
    expect(acceptedHandler).toHaveBeenCalledWith(1, expect.stringContaining("폼 확인"), true);
  });

  it("does not prepare another receipt for a completed correlation", async () => {
    await act(async () => host?.submit("local:1", 1, "폼 확인", "ui-1"));
    await act(async () => host?.submit("local:1", 1, "폼 확인", "ui-1"));
    expect(mocks.prepare).toHaveBeenCalledOnce();
    expect(mocks.send).toHaveBeenCalledOnce();
  });

  it("rejects a different body for an already-bound correlation", async () => {
    await act(async () => host?.submit("local:1", 1, "첫 질문", "ui-1"));
    await act(async () => host?.submit("local:1", 1, "다른 질문", "ui-1"));
    expect(mocks.send).toHaveBeenCalledOnce();
    expect(host?.stateFor("local:1", 1).error).toContain("동일한 내용");
  });

  it("binds concurrent submissions to the first correlation body before refresh", async () => {
    const pendingState = deferred<ReturnType<typeof idle>>();
    mocks.state.mockImplementationOnce(() => pendingState.promise);
    const first = host!.submit("local:1", 1, "첫 질문", "ui-1");
    let second: Promise<void>;
    await act(async () => { second = host!.submit("local:1", 1, "바뀐 질문", "ui-1"); });
    await act(async () => {
      pendingState.resolve(idle());
      await Promise.all([first, second!]);
    });

    expect(mocks.prepare).toHaveBeenCalledOnce();
    expect(mocks.send).toHaveBeenCalledWith(1, "r-1", expect.stringContaining("첫 질문"), "http://localhost:3000", "manual");
  });

  it("requeries and flushes a nonselected task on every idle event", async () => {
    mocks.state.mockResolvedValueOnce(busy(2)).mockResolvedValueOnce(idle(2));
    await act(async () => host?.submit("local:2", 2, "B 질문"));
    expect(mocks.prepare).not.toHaveBeenCalled();
    await act(async () => mocks.handlers.get("preview-workbench://idle")?.({ payload: { taskId: 2 } }));
    expect(mocks.prepare).toHaveBeenCalledOnce();
    expect(mocks.send).toHaveBeenLastCalledWith(2, "r-1", expect.any(String), "http://localhost:3000", "preview_queue");
  });

  it("keeps a prepared receipt and retries the same ID after idle", async () => {
    mocks.send.mockRejectedValueOnce(new Error("lost"));
    await act(async () => host?.submit("local:1", 1, "재시도"));
    expect(host?.stateFor("local:1", 1).inFlight?.requestId).toBe("r-1");
    mocks.send.mockResolvedValueOnce({ requestId: "r-1", status: "accepted", accepted: true, running: true, retryable: false });
    await act(async () => mocks.handlers.get("preview-workbench://idle")?.({ payload: { taskId: 1 } }));
    expect(mocks.prepare).toHaveBeenCalledOnce();
    expect(mocks.send).toHaveBeenLastCalledWith(1, "r-1", expect.any(String), "http://localhost:3000", "manual");
  });

  it("recovers a timed-out relay retry from its bound receipt without a new send", async () => {
    mocks.send.mockRejectedValueOnce(new Error("relay reply lost"));
    await act(async () => host?.submit("local:1", 1, "영수증 확인", "same"));
    mocks.receipt.mockResolvedValueOnce({ requestId: "r-1", status: "accepted", accepted: true, running: true, retryable: false });
    await act(async () => host?.submit("local:1", 1, "영수증 확인", "same"));

    expect(mocks.prepare).toHaveBeenCalledOnce();
    expect(mocks.send).toHaveBeenCalledOnce();
    expect(mocks.receipt).toHaveBeenCalledTimes(2);
    expect(host?.stateFor("local:1", 1)).toMatchObject({ pending: null, inFlight: null, receipt: { correlationId: "same", requestId: "r-1" } });
  });

  it("ignores duplicate receipt recovery after an epoch change invalidates its flight", async () => {
    const receipt = deferred<Awaited<ReturnType<typeof mocks.receipt>>>();
    mocks.send.mockRejectedValueOnce(new Error("reply lost"));
    await act(async () => host?.submit("local:1", 1, "이전 연결 질문", "old"));
    mocks.receipt.mockImplementationOnce(() => receipt.promise);
    const retry = host!.submit("local:1", 1, "이전 연결 질문", "old");
    await act(async () => { await Promise.resolve(); });
    mocks.state.mockResolvedValueOnce({ ...idle(), appEpoch: "new-epoch" });
    await act(async () => host?.refresh("local:1", 1));
    await act(async () => {
      receipt.resolve({ requestId: "r-1", status: "accepted", accepted: true, running: true, retryable: false });
      await retry;
    });

    expect(acceptedHandler).not.toHaveBeenCalled();
    expect(host?.stateFor("local:1", 1)).toMatchObject({ appEpoch: "new-epoch", pending: null, inFlight: null });
  });

  it.each(["rejected", "retired", "invalidated"] as const)("does not mint a request after a %s receipt", async (status) => {
    mocks.send.mockResolvedValueOnce({ requestId: "r-1", status, accepted: false, running: false, retryable: false });
    await act(async () => host?.setDraft("local:1", 1, "실패 질문"));
    await act(async () => host?.submit("local:1", 1, "실패 질문", "failed"));
    await act(async () => mocks.handlers.get("preview-workbench://idle")?.({ payload: { taskId: 1 } }));

    expect(host?.stateFor("local:1", 1)).toMatchObject({ draft: "실패 질문", pending: null, inFlight: null, error: expect.any(String) });
    expect(mocks.prepare).toHaveBeenCalledOnce();
    await act(async () => host?.submit("local:1", 1, "새 질문", "fresh"));
    expect(mocks.prepare).toHaveBeenCalledTimes(2);
    expect(host?.stateFor("local:1", 1).error).toBeNull();
  });

  it("handles a terminal receipt returned during recovery", async () => {
    mocks.send.mockRejectedValueOnce(new Error("lost"));
    mocks.receipt.mockResolvedValueOnce({ requestId: "r-1", status: "rejected", accepted: false, running: false, retryable: false });
    await act(async () => host?.submit("local:1", 1, "복구 실패", "recovery"));
    await act(async () => mocks.handlers.get("preview-workbench://idle")?.({ payload: { taskId: 1 } }));

    expect(host?.stateFor("local:1", 1)).toMatchObject({ draft: "복구 실패", pending: null, inFlight: null, error: expect.any(String) });
    expect(mocks.prepare).toHaveBeenCalledOnce();
  });

  it("restores a rejected queued question and permits a new correlation", async () => {
    mocks.state.mockResolvedValueOnce(busy()).mockResolvedValueOnce(idle());
    mocks.send.mockResolvedValueOnce({ requestId: "r-1", status: "rejected", accepted: false, running: false, retryable: false });
    await act(async () => host?.setDraft("local:1", 1, "탐색 후 바뀐 질문"));
    await act(async () => host?.submit("local:1", 1, "탐색 후 바뀐 질문", "rejected"));
    await act(async () => mocks.handlers.get("preview-workbench://idle")?.({ payload: { taskId: 1 } }));

    expect(host?.stateFor("local:1", 1)).toMatchObject({ draft: "탐색 후 바뀐 질문", pending: null, inFlight: null });
    await act(async () => host?.submit("local:1", 1, "새 질문", "fresh"));
    expect(mocks.prepare).toHaveBeenCalledTimes(2);
  });

  it("clears unchanged queued and accepted drafts while retaining newly typed text", async () => {
    await act(async () => host?.setDraft("local:1", 1, "대기 질문"));
    mocks.state.mockResolvedValueOnce(busy());
    await act(async () => host?.submit("local:1", 1, "대기 질문", "queued"));
    expect(host?.stateFor("local:1", 1).draft).toBe("");

    await act(async () => host?.setDraft("local:1", 1, "수락 질문"));
    await act(async () => host?.submit("local:1", 1, "수락 질문", "accepted-clear"));
    expect(host?.stateFor("local:1", 1).draft).toBe("");

    const send = deferred<Awaited<ReturnType<typeof mocks.send>>>();
    mocks.send.mockImplementationOnce(() => send.promise);
    await act(async () => host?.setDraft("local:1", 1, "보낸 질문"));
    const submitted = host!.submit("local:1", 1, "보낸 질문", "accepted");
    await act(async () => { await Promise.resolve(); });
    await act(async () => host?.setDraft("local:1", 1, "새 초안"));
    await act(async () => {
      send.resolve({ requestId: "r-1", status: "accepted", accepted: true, running: true, retryable: false });
      await submitted;
    });
    expect(host?.stateFor("local:1", 1).draft).toBe("새 초안");
  });

  it("should_keep_newer_idle_state_when_older_busy_and_failed_refreshes_finish_later", async () => {
    const olderBusy = deferred<ReturnType<typeof busy>>();
    const olderFailure = deferred<ReturnType<typeof idle>>();
    mocks.state
      .mockImplementationOnce(() => olderBusy.promise)
      .mockImplementationOnce(() => olderFailure.promise)
      .mockResolvedValueOnce(idle());

    const first = host!.refresh("local:1", 1);
    const second = host!.refresh("local:1", 1);
    await act(async () => host!.refresh("local:1", 1));
    await act(async () => {
      olderBusy.resolve(busy());
      olderFailure.reject(new Error("stale query failed"));
      await Promise.all([first, second]);
    });

    expect(host?.stateFor("local:1", 1)).toMatchObject({ busy: "idle", error: null });
  });

  it("should_execute_a_correlation_once_when_repeated_before_and_after_acceptance", async () => {
    mocks.state.mockResolvedValueOnce(busy()).mockResolvedValueOnce(idle());
    await act(async () => host?.submit("local:1", 1, "첫 질문", "ui-1"));
    await act(async () => host?.submit("local:1", 1, "첫 질문", "ui-1"));
    await act(async () => mocks.handlers.get("preview-workbench://idle")?.({ payload: { taskId: 1 } }));
    await act(async () => host?.submit("local:1", 1, "바뀐 질문", "ui-1"));

    expect(mocks.prepare).toHaveBeenCalledOnce();
    expect(mocks.send).toHaveBeenCalledOnce();
  });

  it("should_preserve_a_new_pending_question_when_an_older_send_fails", async () => {
    const failedSend = deferred<Awaited<ReturnType<typeof mocks.send>>>();
    mocks.send.mockImplementationOnce(() => failedSend.promise);
    const firstSubmission = host!.submit("local:1", 1, "이전 질문", "old");
    await act(async () => { await Promise.resolve(); });
    await act(async () => host?.submit("local:1", 1, "새 질문", "new"));
    await act(async () => {
      failedSend.reject(new Error("send failed"));
      await firstSubmission;
    });

    expect(host?.stateFor("local:1", 1).pending).toMatchObject({ correlationId: "new", message: "새 질문" });
  });

  it("should_not_accept_or_restore_a_question_when_its_task_is_deleted_while_receipt_is_pending", async () => {
    const pendingReceipt = deferred<Awaited<ReturnType<typeof mocks.receipt>>>();
    mocks.send.mockRejectedValueOnce(new Error("response lost"));
    mocks.receipt.mockImplementationOnce(() => pendingReceipt.promise);
    const submission = host!.submit("local:1", 1, "삭제될 질문", "deleted");
    await act(async () => { await Promise.resolve(); });
    await renderTasks([]);
    await act(async () => {
      pendingReceipt.resolve({ requestId: "r-1", status: "accepted", accepted: true, running: true, retryable: false });
      await submission;
    });
    await renderTasks(tasks);

    expect(acceptedHandler).not.toHaveBeenCalled();
    expect(host?.stateFor("local:1", 1)).toMatchObject({ pending: null, inFlight: null });
    await act(async () => host?.submit("local:1", 1, "삭제될 질문", "deleted"));
    expect(mocks.prepare).toHaveBeenCalledTimes(2);
  });

  it("disposes terminal work while a receipt is pending", async () => {
    const send = deferred<Awaited<ReturnType<typeof mocks.send>>>();
    mocks.send.mockImplementationOnce(() => send.promise);
    await act(async () => host?.setDraft("local:1", 1, "폐기 질문"));
    const submission = host!.submit("local:1", 1, "폐기 질문", "terminal");
    await act(async () => { await Promise.resolve(); });
    await renderTasks([{ ...tasks[0], supported: false, terminal: true, unsupportedReason: "완료된 작업입니다." }]);
    await act(async () => {
      send.resolve({ requestId: "r-1", status: "accepted", accepted: true, running: true, retryable: false });
      await submission;
    });

    expect(acceptedHandler).not.toHaveBeenCalled();
    expect(host?.stateFor("local:1", 1)).toMatchObject({ draft: "", pending: null, inFlight: null, supported: false });
  });

  it("does not accept an existing flight after support is removed", async () => {
    const send = deferred<Awaited<ReturnType<typeof mocks.send>>>();
    mocks.send.mockImplementationOnce(() => send.promise);
    const submission = host!.submit("local:1", 1, "비지원 질문", "unsupported");
    await act(async () => { await Promise.resolve(); });
    await renderTasks([{ ...tasks[0], supported: false, unsupportedReason: "지원하지 않습니다." }]);
    await act(async () => {
      send.resolve({ requestId: "r-1", status: "accepted", accepted: true, running: true, retryable: false });
      await submission;
    });

    expect(acceptedHandler).not.toHaveBeenCalled();
  });

  it("should_use_the_refreshed_url_when_a_busy_queue_becomes_idle", async () => {
    mocks.state
      .mockResolvedValueOnce({ ...busy(), url: "http://localhost:3000/old" })
      .mockResolvedValueOnce({ ...idle(), url: "http://localhost:3000/new" });
    await act(async () => host?.submit("local:1", 1, "URL 갱신", "url-change"));
    await act(async () => mocks.handlers.get("preview-workbench://idle")?.({ payload: { taskId: 1 } }));

    expect(mocks.send).toHaveBeenLastCalledWith(1, "r-1", expect.any(String), "http://localhost:3000/new", "preview_queue");
  });

  it("should_submit_while_taken_over_and_preserve_queue_and_manual_sources", async () => {
    mocks.state
      .mockResolvedValueOnce({ ...busy(), takenOver: true })
      .mockResolvedValueOnce({ ...idle(), takenOver: true })
      .mockResolvedValueOnce({ ...idle(), takenOver: true });
    await act(async () => host?.submit("local:1", 1, "대기 질문", "queued"));
    await act(async () => mocks.handlers.get("preview-workbench://idle")?.({ payload: { taskId: 1 } }));
    await act(async () => host?.submit("local:1", 1, "직접 질문", "manual"));

    expect(mocks.send).toHaveBeenCalledTimes(2);
    expect(mocks.send).toHaveBeenNthCalledWith(1, 1, "r-1", expect.any(String), "http://localhost:3000", "preview_queue");
    expect(mocks.send).toHaveBeenNthCalledWith(2, 1, "r-1", expect.any(String), "http://localhost:3000", "manual");
  });

  it("queues a supported local request when a refresh leaves busy unknown", async () => {
    mocks.state.mockRejectedValueOnce(new Error("retry"));
    await act(async () => host?.setDraft("local:1", 1, "대기 질문"));
    await act(async () => host?.submit("local:1", 1, "대기 질문", "unknown"));

    expect(host?.stateFor("local:1", 1)).toMatchObject({ busy: "unknown", pending: { source: "preview_queue" }, draft: "", error: expect.any(String) });
  });

  it("uses control events only for display and pauses a closed preview", async () => {
    await act(async () => mocks.handlers.get("designmode://control")?.({ payload: { task_id: 1, op: "click", target: "완료", changed: true, url: "http://localhost:3000" } }));
    expect(host?.stateFor("local:1", 1).lastAction).toContain("click 완료");
    await act(async () => host?.submit("local:1", 1, "대기"));
    await act(async () => mocks.handlers.get("designmode://closed")?.({ payload: 1 }));
    expect(host?.stateFor("local:1", 1).url).toBeNull();
  });
});
