// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../../lib/ipc";
import {
  useDeferredTaskRemoval,
  type DeferredTaskRemoval,
  type DeferredTaskRemovalOptions,
} from "./useDeferredTaskRemoval";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const GRACE_MS = 10_000;

const makeTask = (id: number, state = "Running"): Task => ({
  id,
  host: "local",
  repo: "/workspace/alpha",
  branch: `feature/task-${id}`,
  base: "main",
  worktree_path: `/workspace/alpha/.praxis/worktrees/task-${id}`,
  instruction: `작업 ${id}`,
  state,
  created_at: id,
  updated_at: id,
  mode: "conversation",
});

let container: HTMLDivElement | null = null;
let root: Root | null = null;
let hook: DeferredTaskRemoval;
let commit: ReturnType<typeof vi.fn>;
let onSchedule: ReturnType<typeof vi.fn>;
let onUndo: ReturnType<typeof vi.fn>;
let probe: Task | undefined;

function Harness(props: { options: DeferredTaskRemovalOptions; probe?: Task }) {
  hook = useDeferredTaskRemoval(props.options);
  return <div data-pending={hook.pending.length} data-masked={hook.isPending(props.probe ?? makeTask(0))} />;
}

const render = async (): Promise<void> => {
  await act(async () => {
    root?.render(<Harness options={{ commit, onSchedule, onUndo, graceMs: GRACE_MS }} probe={probe} />);
  });
};

const schedule = async (task: Task): Promise<void> => {
  await act(async () => {
    hook.schedule(task);
  });
};

const pressUndo = async (): Promise<KeyboardEvent> => {
  const event = new KeyboardEvent("keydown", {
    key: "z",
    metaKey: true,
    bubbles: true,
    cancelable: true,
  });
  await act(async () => {
    window.dispatchEvent(event);
  });
  return event;
};

const advance = async (ms: number): Promise<void> => {
  await act(async () => {
    vi.advanceTimersByTime(ms);
  });
};

beforeEach(async () => {
  vi.useFakeTimers();
  commit = vi.fn(async () => {});
  onSchedule = vi.fn();
  onUndo = vi.fn();
  probe = undefined;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await render();
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  container = null;
  root = null;
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("세션 삭제 유예", () => {
  it("예약만으로는 아무것도 지우지 않고, 유예가 끝나야 확정한다", async () => {
    const task = makeTask(11);
    await schedule(task);

    expect(commit).not.toHaveBeenCalled();
    expect(hook.isPending(task)).toBe(true);
    expect(onSchedule).toHaveBeenCalledWith(task);

    await advance(GRACE_MS - 1);
    expect(commit).not.toHaveBeenCalled();

    await advance(1);
    expect(commit).toHaveBeenCalledWith(task);
    expect(hook.isPending(task)).toBe(false);
  });

  it("⌘Z는 유예 중인 삭제를 취소한다 — 실행 중인 세션이 중단되지 않는다", async () => {
    const task = makeTask(11);
    await schedule(task);
    await advance(GRACE_MS / 2);

    await pressUndo();
    expect(onUndo).toHaveBeenCalledWith(task);
    expect(hook.pending).toEqual([]);

    await advance(GRACE_MS * 2);
    expect(commit).not.toHaveBeenCalled();
  });

  it("연달아 지웠으면 ⌘Z는 가장 최근 것부터 되살린다", async () => {
    await schedule(makeTask(11));
    await schedule(makeTask(12));

    await pressUndo();
    expect(onUndo.mock.calls[0]?.[0]?.id).toBe(12);
    expect(hook.pending.map((entry) => entry.task.id)).toEqual([11]);

    await advance(GRACE_MS);
    expect(commit).toHaveBeenCalledOnce();
    expect(commit).toHaveBeenCalledWith(expect.objectContaining({ id: 11 }));
  });

  it("같은 작업을 두 번 눌러도 카운트다운은 하나만 돈다", async () => {
    const task = makeTask(11);
    await schedule(task);
    await advance(GRACE_MS / 2);
    await schedule(task);

    expect(hook.pending).toHaveLength(1);
    await advance(GRACE_MS / 2);
    expect(commit).toHaveBeenCalledOnce();
  });

  it("이미 예약된 작업을 다시 버려도 화면은 접는다 — 죽은 버튼이 되지 않는다", async () => {
    const task = makeTask(11);
    await schedule(task);
    const deadline = hook.pending[0]?.deadline;
    await advance(GRACE_MS / 2);
    await schedule(task);

    // Quick Open·알림은 예약된 세션도 다시 연다. 거기서 다시 눌렀을 때 조용히 돌아가면
    // 버튼이 고장 난 것처럼 보이므로 화면은 접되, 남은 시간은 늘려 주지 않는다.
    expect(onSchedule).toHaveBeenCalledTimes(2);
    expect(hook.pending[0]?.deadline).toBe(deadline);
  });

  it("입력 중일 때의 ⌘Z는 그쪽 되돌리기다 — 삭제를 가로채지 않는다", async () => {
    await schedule(makeTask(11));
    const input = document.createElement("textarea");
    document.body.appendChild(input);
    input.focus();

    const event = await pressUndo();

    expect(event.defaultPrevented).toBe(false);
    expect(onUndo).not.toHaveBeenCalled();
    expect(hook.pending).toHaveLength(1);
    input.remove();
  });

  it("되돌릴 삭제가 없으면 ⌘Z를 삼키지 않는다", async () => {
    const event = await pressUndo();

    expect(event.defaultPrevented).toBe(false);
    expect(onUndo).not.toHaveBeenCalled();
  });

  it("창이 닫히면 예약은 확정이 아니라 취소된다", async () => {
    await schedule(makeTask(11));

    await act(async () => root?.unmount());
    await advance(GRACE_MS * 2);

    expect(commit).not.toHaveBeenCalled();
  });

  it("확정 IPC가 끝날 때까지 목록 마스크를 유지하고, 같은 id의 다른 호스트는 가리지 않는다", async () => {
    let resolveCommit: (() => void) | undefined;
    commit.mockImplementationOnce(() => new Promise<void>((resolve) => {
      resolveCommit = resolve;
    }));
    const local = makeTask(11);
    const remote = { ...makeTask(11), host: "runner-1" };

    await schedule(local);
    await advance(GRACE_MS);

    expect(hook.pending).toEqual([]);
    expect(hook.isPending(local)).toBe(true);
    expect(hook.isPending(remote)).toBe(false);

    await act(async () => resolveCommit?.());
    expect(hook.isPending(local)).toBe(false);
  });

  it("늦게 도착한 확정 실패도 숨김을 해제해 다시 예약할 수 있다", async () => {
    const task = makeTask(11);
    let rejectCommit!: (error: Error) => void;
    commit.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => {
      rejectCommit = reject;
    }));
    probe = task;
    await render();

    await schedule(task);
    await advance(GRACE_MS);
    expect(container?.firstElementChild?.getAttribute("data-masked")).toBe("true");

    await act(async () => rejectCommit(new Error("IPC 실패")));

    expect(hook.isPending(task)).toBe(false);
    expect(container?.firstElementChild?.getAttribute("data-masked")).toBe("false");
    await schedule(task);
    expect(hook.pending).toHaveLength(1);
  });
});
