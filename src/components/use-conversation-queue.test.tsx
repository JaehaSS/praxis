// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task, ConvoEvent } from "../lib/ipc";

const mocks = vi.hoisted(() => ({
  transports: new Map<string, unknown>(),
  listeners: new Map<string, (event: { payload: ConvoEvent }) => void>(),
  activity: vi.fn(async (): Promise<{ task_id: number }[]> => []),
  unlisten: vi.fn(),
}));
vi.mock("../lib/ipc", () => ({ taskActivity: mocks.activity }));
vi.mock("../lib/transport", () => ({
  LOCAL_HOST: "local",
  taskKey: (ref: { host: string; id: number }) => `${ref.host}:${ref.id}`,
  hasHost: (host: string) => mocks.transports.has(host),
  getTransport: (host: string) => {
    const transport = mocks.transports.get(host);
    if (!transport) throw new Error("disconnected");
    return transport;
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, callback: (event: { payload: ConvoEvent }) => void) => {
  mocks.listeners.set(name, callback);
  return mocks.unlisten;
}) }));

import { useConversationQueue } from "./use-conversation-queue";
import { ConversationSubmitter } from "../lib/conversation-submit";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let node: HTMLDivElement;
let controller: ReturnType<typeof useConversationQueue>;
const onAccepted = vi.fn();
const local = { host: "local", id: 7 };
const remote = { host: "ssh", id: 7 };
function transport(host: string, state = "Running") {
  const task = { host, id: 7, mode: "conversation", state } as Task;
  const api = {
    taskList: vi.fn(async () => [task]),
    conversationSubmit: vi.fn(async (_task: number, id: string) => ({ request_id: id, status: "accepted" })),
    conversationReceipt: vi.fn(async (_task: number, id: string) => ({ request_id: id, status: "unknown" })),
  };
  mocks.transports.set(host, api);
  return { task, api };
}
async function mount() {
  const submitter = new ConversationSubmitter();
  const tasks: Task[] = [];
  function Harness() {
    controller = useConversationQueue({ submitter, tasks, onSending: vi.fn(), onAccepted, onFailed: vi.fn() });
    return null;
  }
  await act(async () => root.render(<Harness />));
}
beforeEach(() => {
  vi.useFakeTimers();
  mocks.transports.clear();
  mocks.listeners.clear();
  vi.clearAllMocks();
  mocks.activity.mockResolvedValue([]);
  node = document.createElement("div");
  root = createRoot(node);
});
afterEach(async () => {
  await act(async () => root.unmount());
  vi.useRealTimers();
});

describe("app-owned conversation queue delivery", () => {
  it("waits for the local reservation to end, then sends with no selected session", async () => {
    const { task, api } = transport("local");
    await mount();
    await act(async () => { controller.queue.enqueue(local, "next", ["a.png"]); });
    await act(async () => vi.advanceTimersByTimeAsync(1_000));
    expect(api.conversationSubmit).not.toHaveBeenCalled();
    task.state = "AwaitingReview";
    mocks.activity.mockResolvedValue([{ task_id: 7 }]);
    await act(async () => vi.advanceTimersByTimeAsync(1_000));
    expect(api.conversationSubmit).not.toHaveBeenCalled();
    mocks.activity.mockResolvedValue([]);
    await act(async () => vi.advanceTimersByTimeAsync(1_000));
    expect(api.conversationSubmit).toHaveBeenCalledWith(7, expect.any(String), "next", ["a.png"]);
    expect(onAccepted).toHaveBeenCalledOnce();
  });

  it("routes remote tasks to their host without calling local activity IPC", async () => {
    const localApi = transport("local", "AwaitingReview").api;
    const remoteApi = transport("ssh", "AwaitingReview").api;
    await mount();
    await act(async () => {
      controller.queue.enqueue(remote, "remote next", []);
      await controller.flush();
    });
    expect(remoteApi.conversationSubmit).toHaveBeenCalledWith(7, expect.any(String), "remote next", []);
    expect(localApi.taskList).not.toHaveBeenCalled();
    expect(localApi.conversationSubmit).not.toHaveBeenCalled();
    expect(mocks.activity).not.toHaveBeenCalled();
  });

  it("pauses on turn errors, retaining requests until explicit resume", async () => {
    const { api } = transport("local", "AwaitingReview");
    await mount();
    await act(async () => {
      controller.queue.enqueue(local, "next", []);
      mocks.listeners.get("convo://event")!({ payload: { id: 7, kind: "result", is_error: true } as ConvoEvent });
      await controller.flush();
    });
    expect(api.conversationSubmit).not.toHaveBeenCalled();
    expect(controller.queue.snapshot("local:7").paused).toBe(true);
    await act(async () => { controller.queue.resume("local:7"); await controller.flush(); });
    expect(api.conversationSubmit).toHaveBeenCalledOnce();
  });

  it("preserves requests on disconnect and task termination without creating another task", async () => {
    const { task, api } = transport("ssh");
    await mount();
    await act(async () => { controller.queue.enqueue(remote, "next", []); });
    mocks.transports.delete("ssh");
    await act(async () => controller.flush());
    expect(controller.queue.snapshot("ssh:7").reason).toContain("연결");
    mocks.transports.set("ssh", api);
    task.state = "Done";
    await act(async () => { controller.queue.resume("ssh:7"); await controller.flush(); });
    expect(controller.queue.snapshot("ssh:7").reason).toContain("종료");
    expect(api.conversationSubmit).not.toHaveBeenCalled();
  });

  it("performs no queries for empty queues and stops timers/listeners on unmount", async () => {
    const { api } = transport("local", "AwaitingReview");
    await mount();
    await act(async () => vi.advanceTimersByTimeAsync(5_000));
    expect(api.taskList).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    expect(vi.getTimerCount()).toBe(0);
    expect(mocks.unlisten).toHaveBeenCalledOnce();
  });
});
