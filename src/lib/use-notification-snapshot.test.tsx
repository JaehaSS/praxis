// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  snapshot: vi.fn(), source: vi.fn(), remoteSource: vi.fn(), ingest: vi.fn(), reconcile: vi.fn(), listen: vi.fn(),
  listHosts: vi.fn(), transport: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("./transport", () => ({ LOCAL_HOST: "local", listHosts: mocks.listHosts, getTransport: mocks.transport }));
vi.mock("./transport/runner", () => ({ RunnerTransport: class RunnerTransport {} }));
vi.mock("./notifications", () => ({
  notificationSnapshot: mocks.snapshot,
  notificationSourcePage: mocks.source,
  notificationIngest: mocks.ingest,
  notificationReconcile: mocks.reconcile,
}));

import { useNotificationCollector, useNotificationSnapshot } from "./use-notification-snapshot";
import type { NotificationSnapshot, SourcePage } from "./notifications";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const saved: NotificationSnapshot = {
  enabled: true, delivery_error: null, items: [],
  sources: [{ host: "local", source_id: "source", cursor: 2, warning: null }],
};
const page = (cursor: number, watermark: number): SourcePage => ({ source_id: "source", after: cursor - 1, cursor, watermark, results: [] });

function Harness() {
  const state = useNotificationSnapshot();
  collector = useNotificationCollector(state.snapshot, state.setSnapshot, state.setError);
  observed = state.snapshot;
  return <output>{state.error}</output>;
}

let root: Root;
let host: HTMLDivElement;
let persistedCursor: number;
let collector: ReturnType<typeof useNotificationCollector> | null = null;
let observed: NotificationSnapshot | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  mocks.listen.mockReset();
  mocks.listen.mockResolvedValue(() => {});
  mocks.source.mockReset();
  mocks.remoteSource.mockReset();
  mocks.reconcile.mockReset();
  mocks.reconcile.mockResolvedValue(saved);
  mocks.listHosts.mockReturnValue(["local"]);
  mocks.transport.mockImplementation((name: string) => name === "remote"
    ? { notificationSourcePage: mocks.remoteSource, taskList: vi.fn() }
    : { taskList: vi.fn() });
  mocks.ingest.mockReset();
  persistedCursor = 2;
  mocks.ingest.mockImplementation(async (_host, sourcePage: SourcePage) => {
    persistedCursor = sourcePage.cursor;
    return { ...saved, sources: [{ ...saved.sources[0], cursor: persistedCursor }] };
  });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  collector = null;
  observed = null;
  vi.useRealTimers();
});

describe("notification collector", () => {
  it("persisted snapshot이 오기 전에는 baseline을 요청하지 않는다", async () => {
    let resolve!: (value: NotificationSnapshot) => void;
    mocks.snapshot.mockReturnValue(new Promise((done) => { resolve = done; }));
    await act(async () => root.render(<Harness />));
    expect(mocks.source).not.toHaveBeenCalled();
    await act(async () => resolve(saved));
    expect(mocks.source).toHaveBeenCalledWith(2);
  });

  it("이벤트 구독 실패를 표시하고 unmount 뒤 늦은 load를 무시한다", async () => {
    let resolve!: (value: NotificationSnapshot) => void;
    mocks.snapshot.mockReturnValue(new Promise((done) => { resolve = done; }));
    mocks.listen.mockRejectedValueOnce(new Error("events unavailable"));
    await act(async () => root.render(<Harness />));
    await act(async () => { await Promise.resolve(); });
    expect(host.textContent).toContain("events unavailable");
    await act(async () => root.unmount());
    await act(async () => resolve(saved));
    expect(host.textContent).toBe("");
  });

  it("복구 watermark까지는 무음으로 넣고 뒤 페이지부터 발송한다", async () => {
    mocks.snapshot.mockResolvedValue(saved);
    mocks.source.mockResolvedValueOnce(page(3, 4)).mockResolvedValueOnce(page(4, 4)).mockResolvedValueOnce(page(5, 5));
    await act(async () => root.render(<Harness />));
    await act(async () => {});
    expect(mocks.ingest.mock.calls.map((call) => call[2])).toEqual([false, false]);
    await act(async () => vi.advanceTimersByTime(750));
    expect(mocks.ingest.mock.calls[2][2]).toBe(true);
  });

  it("다음 페이지에서 recovery watermark를 넘으면 이전 결과와 새 결과를 나눠 넣는다", async () => {
    mocks.snapshot.mockResolvedValue(saved);
    mocks.source.mockImplementation(async (after: number | null) => {
      if (after === 2) return {
        ...page(3, 4), after: 2,
        results: [{ sequence: 3, task_id: 1, ts: 1, kind: "result", title: "old", repo: "/work/demo" }],
      };
      if (after === 3) return {
        ...page(5, 5), after: 3,
        results: [
          { sequence: 4, task_id: 1, ts: 2, kind: "result", title: "old", repo: "/work/demo" },
          { sequence: 5, task_id: 1, ts: 3, kind: "result", title: "new", repo: "/work/demo" },
        ],
      };
      if (after === 4) return {
        ...page(5, 5), after: 4,
        results: [{ sequence: 5, task_id: 1, ts: 3, kind: "result", title: "new", repo: "/work/demo" }],
      };
      throw new Error(`unexpected cursor ${after}`);
    });
    await act(async () => root.render(<Harness />));
    await act(async () => {});
    expect(mocks.source.mock.calls.map(([after]) => after)).toEqual([2, 3, 4]);
    expect(mocks.ingest.mock.calls.map((call) => call[1].cursor)).toEqual([3, 4, 5]);
    expect(mocks.ingest.mock.calls.map((call) => call[2])).toEqual([false, false, true]);
    expect(mocks.ingest.mock.calls[1][1].results).toEqual([expect.objectContaining({ sequence: 4 })]);
    expect(persistedCursor).toBe(5);
  });

  it("느린 원격 reconcile이 로컬 ingest를 막지 않는다", async () => {
    let finishRemote!: (tasks: { id: number }[]) => void;
    const remoteTaskList = new Promise<{ id: number }[]>((resolve) => { finishRemote = resolve; });
    mocks.transport.mockImplementation((name: string) => name === "remote"
      ? { notificationSourcePage: mocks.remoteSource, taskList: () => remoteTaskList }
      : { taskList: vi.fn() });
    mocks.snapshot.mockResolvedValue(saved);
    mocks.source.mockResolvedValueOnce(page(2, 2)).mockResolvedValue(page(3, 3));
    await act(async () => root.render(<Harness />));
    await act(async () => {});
    mocks.ingest.mockClear();
    collector?.reconcile("remote");
    await act(async () => vi.advanceTimersByTime(750));
    expect(mocks.ingest).toHaveBeenCalledWith("local", expect.anything(), true);
    await act(async () => finishRemote([]));
  });

  it("변화 없는 틱은 ingest도 setSnapshot도 부르지 않는다", async () => {
    mocks.snapshot.mockResolvedValue(saved);
    mocks.source.mockResolvedValue(page(2, 2));
    await act(async () => root.render(<Harness />));
    await act(async () => {});
    const ingested = mocks.ingest.mock.calls.length;
    const before = observed;
    await act(async () => vi.advanceTimersByTime(750));
    expect(mocks.ingest.mock.calls.length).toBe(ingested);
    expect(observed).toBe(before);
  });
});
