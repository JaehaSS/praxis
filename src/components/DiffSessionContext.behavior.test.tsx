// @vitest-environment jsdom

import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  annotationsList: vi.fn(async () => []),
  diffHunks: vi.fn(async (): Promise<DiffHunk[]> => []),
  taskDiff: vi.fn(),
}));

vi.mock("../lib/ipc", () => ({
  annotationSave: vi.fn(),
  annotationsList: mocks.annotationsList,
  annotationsResend: vi.fn(),
  diffHunks: mocks.diffHunks,
  partialApply: vi.fn(),
  partialRollback: vi.fn(),
  taskDiff: mocks.taskDiff,
}));

import { DiffSessionProvider, DiffSessionScope, useDiffSession } from "./DiffSessionContext";
import type { DiffHunk } from "../lib/ipc";
import type { TaskRef } from "../lib/transport";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** 렌더마다 스냅샷의 파일 수를 기록한다. `null`은 "다시 불러오는 중"이라는 뜻이다. */
const seen: (number | null)[] = [];

function Subscriber({ on }: { on: boolean }) {
  const { data, retain } = useDiffSession();
  seen.push(data.files === null ? null : data.files.length);
  useEffect(() => {
    if (!on) return;
    return retain();
  }, [on, retain]);
  return null;
}

/** 마지막으로 그려진 세션 값. 작업을 옮긴 뒤 무엇이 남았는지 보려고 밖으로 뺀다. */
const latest: {
  files: string[] | null;
  selection: string[];
  viewed: string[];
  toggleViewed: (path: string) => void;
} = { files: null, selection: [], viewed: [], toggleViewed: () => {} };

function Probe({ on }: { on: boolean }) {
  const { data, partial, viewed, toggleViewed, retain } = useDiffSession();
  latest.files = data.files === null ? null : data.files.map((f) => f.path);
  latest.selection = [...partial.selection];
  latest.viewed = Object.keys(viewed);
  latest.toggleViewed = toggleViewed;
  useEffect(() => {
    if (!on) return;
    return retain();
  }, [on, retain]);
  return null;
}

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const render = async (on: boolean) => {
  await act(async () => {
    root?.render(
      <DiffSessionProvider task={{ host: "local", id: 5 }} openDiff={() => {}}>
        <Subscriber on={on} />
      </DiffSessionProvider>,
    );
    await Promise.resolve();
  });
};

const renderScope = async (task: TaskRef, on: boolean) => {
  await act(async () => {
    root?.render(
      <DiffSessionScope task={task} openDiff={() => {}}>
        <Probe on={on} />
      </DiffSessionScope>,
    );
    await Promise.resolve();
  });
};

beforeEach(() => {
  vi.useFakeTimers();
  seen.length = 0;
  localStorage.clear();
  mocks.taskDiff.mockResolvedValue({
    files: [{ path: "src/a.ts", status: "M", patch: "@@ -1 +1 @@\n+const b = 2;" }],
    baseline: { kind: "pinned" as const },
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  container = null;
  root = null;
  vi.clearAllMocks();
  vi.useRealTimers();
});

describe("DiffSessionContext 폴링 게이트 (AC-7 · F-9)", () => {
  it("구독자가 없으면 조회하지 않고, 재개는 스냅샷을 버리지 않는 조용한 갱신이다", async () => {
    await render(false);
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(mocks.taskDiff).not.toHaveBeenCalled();

    await render(true);
    expect(mocks.taskDiff).toHaveBeenCalledTimes(1);

    // 게이트가 닫히면 타이머도 멈춘다 — 숨은 화면이 IPC를 계속 두드리지 않는다.
    await render(false);
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(mocks.taskDiff).toHaveBeenCalledTimes(1);

    // 재개할 때 files=null을 거치면 hunkKey가 흔들려 부분 적용 선택이 초기화된다.
    seen.length = 0;
    await render(true);
    expect(mocks.taskDiff).toHaveBeenCalledTimes(2);
    expect(seen).not.toContain(null);
  });
});

const HUNK_A: DiffHunk = {
  id: "A-h1",
  path: "src/a.ts",
  old_range: [1, 1],
  new_range: [1, 1],
  protected: false,
  committed: false,
  risk: "low",
  lines: [{ kind: "add", text: "const b = 2;" }],
};

/**
 * 작업을 옮기면 세션이 통째로 새로 만들어진다(설계 F-5). 게이트가 닫힌 채 옮기는 것이
 * 이 검사의 요점이다 — 폴링이 돌지 않으므로 초기화해 주는 것은 재마운트뿐이다.
 */
describe("DiffSessionScope 작업 전환 (F-5)", () => {
  it("같은 id의 다른 호스트로 옮겨도 앞 작업의 스냅샷·부분 적용 선택이 넘어오지 않는다", async () => {
    mocks.diffHunks.mockResolvedValue([HUNK_A]);
    await renderScope({ host: "local", id: 7 }, true);
    expect(latest.files).toEqual(["src/a.ts"]);
    expect(latest.selection).toEqual(["A-h1"]);

    // 변경 목록을 닫는다 — 여기서 옮기면 새 스냅샷을 부르지 않는다.
    await renderScope({ host: "local", id: 7 }, false);
    mocks.taskDiff.mockClear();

    await renderScope({ host: "remote", id: 7 }, false);

    expect(mocks.taskDiff).not.toHaveBeenCalled();
    expect(latest.files).toBeNull();
    // 앞 작업의 hunk id가 남으면 그 id가 새 작업의 부분 적용으로 실려 나간다.
    expect(latest.selection).toEqual([]);
  });

  it("작업이 바뀌면 확인함도 새 작업의 것으로 갈아 끼운다", async () => {
    mocks.diffHunks.mockResolvedValue([HUNK_A]);
    await renderScope({ host: "local", id: 7 }, true);
    await act(async () => latest.toggleViewed("src/a.ts"));
    expect(latest.viewed).toEqual(["src/a.ts"]);

    await renderScope({ host: "local", id: 8 }, false);

    expect(latest.viewed).toEqual([]);
  });
});
