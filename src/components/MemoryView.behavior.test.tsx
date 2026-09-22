// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Memory } from "../lib/ipc";
import { HostScopeProvider } from "../lib/host-scope";

const mocks = vi.hoisted(() => ({
  memoryList: vi.fn(),
  proposalList: vi.fn(),
  proposalApply: vi.fn(),
  captureEnabledGet: vi.fn(),
  memoryArchive: vi.fn(),
  memoryPurge: vi.fn(),
  memoryUsages: vi.fn(),
  contextReport: vi.fn(),
}));

vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  memoryList: mocks.memoryList,
  proposalList: mocks.proposalList,
  proposalApply: mocks.proposalApply,
  captureEnabledGet: mocks.captureEnabledGet,
  memoryArchive: mocks.memoryArchive,
  memoryPurge: mocks.memoryPurge,
  memoryUsages: mocks.memoryUsages,
  contextReport: mocks.contextReport,
}));

import { MemoryView } from "./MemoryView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

function memory(id: number, status: Memory["status"], content: string): Memory {
  return {
    id,
    tier: "project",
    scope_key: "/repo/a",
    kind: "observation",
    content,
    source_session: null,
    confidence: 0,
    usage_count: 0,
    last_used: null,
    created_at: id,
    knowledge_type: "observation",
    status,
    current_version: 1,
    utility_score: 0,
    review_due_at: null,
    verified_at: null,
    stale_at: null,
    archived_at: null,
    dormant: false,
  };
}

function button(label: string): HTMLButtonElement | undefined {
  return Array.from(container.querySelectorAll("button")).find(
    (item) => item.textContent === label,
  );
}

/** 필터 세그먼트는 라벨 뒤에 개수를 병기한다 — 라벨만 떼어 찾는다. */
function segment(label: string): HTMLButtonElement | undefined {
  return Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]')).find(
    (item) => (item.textContent ?? "").replace(/[\d,]+$/, "").trim() === label,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.memoryList.mockResolvedValue([
    memory(1, "candidate", "새 후보 메모리"),
    memory(2, "verified", "승인된 메모리"),
    memory(3, "archived", "보관된 메모리"),
  ]);
  mocks.proposalList.mockResolvedValue([
    {
      id: 7,
      repo: "/repo/a",
      kind: "reflection",
      content: "검증 명령을 먼저 실행한다",
      status: "proposed",
      source_session: "session-1",
      created_at: 1,
      decided_at: null,
      applied_memory_id: null,
    },
  ]);
  mocks.proposalApply.mockResolvedValue(undefined);
  mocks.captureEnabledGet.mockResolvedValue(false);
  mocks.memoryArchive.mockResolvedValue(undefined);
  mocks.memoryPurge.mockResolvedValue(undefined);
  mocks.memoryUsages.mockResolvedValue([]);
  mocks.contextReport.mockResolvedValue({
    vendors: [],
    injected: [],
    capture_enabled: false,
    memory_count: 0,
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});

describe("MemoryView self-improvement handoff", () => {
  it("opens the candidate filter after creating a memory candidate", async () => {
    await act(async () => {
      root.render(<MemoryView />);
      await Promise.resolve();
    });
    await act(async () => button("자기개선")?.click());
    await act(async () => button("메모리 후보 만들기")?.click());

    expect(mocks.proposalApply).toHaveBeenCalledWith(7);
    // 탭은 Tabs, 필터는 FilterSegment — 둘 다 aria-selected로 활성을 말한다.
    expect(segment("메모리")?.getAttribute("aria-selected")).toBe("true");
    expect(segment("후보")?.getAttribute("aria-selected")).toBe("true");
    expect(container.textContent).toContain("새 후보 메모리");
    expect(container.textContent).not.toContain("승인된 메모리");
  });

  it("archives only the current lifecycle-filter result", async () => {
    const confirmed = vi.spyOn(window, "confirm").mockReturnValue(true);
    await act(async () => {
      root.render(<MemoryView />);
      await Promise.resolve();
    });

    await act(async () => segment("후보")?.click());
    await act(async () => button("현재 결과 1건 보관")?.click());

    expect(confirmed).toHaveBeenCalledWith(expect.stringContaining("현재 결과 1건"));
    // archive는 host를 첫 인자로 받는다 — 세션별 환경(ADR 0133) 이후의 계약이다.
    expect(mocks.memoryArchive.mock.calls).toEqual([["local", 1]]);
    confirmed.mockRestore();
  });
});

describe("MemoryView permanent delete", () => {
  it("offers permanent delete only inside the archived filter", async () => {
    await act(async () => {
      root.render(<MemoryView />);
      await Promise.resolve();
    });

    // 기본 진입(활성)에서는 보관 항목 자체가 보이지 않으므로 삭제도 닿을 수 없다.
    expect(button("영구 삭제")).toBeUndefined();

    await act(async () => segment("보관됨")?.click());

    expect(container.textContent).toContain("보관된 메모리");
    expect(button("영구 삭제")).toBeDefined();
    // 되돌릴 수 없는 삭제와 보관을 같은 줄에 나란히 두지 않는다.
    expect(button("보관")).toBeUndefined();
  });

  it("purges the archived memory after the warning is accepted", async () => {
    const confirmed = vi.spyOn(window, "confirm").mockReturnValue(true);
    await act(async () => {
      root.render(<MemoryView />);
      await Promise.resolve();
    });
    await act(async () => segment("보관됨")?.click());
    await act(async () => button("영구 삭제")?.click());

    expect(confirmed).toHaveBeenCalledWith(expect.stringContaining("되돌릴 수 없습니다"));
    expect(mocks.memoryPurge.mock.calls).toEqual([["local", 3]]);
    confirmed.mockRestore();
  });

  it("sends nothing when the warning is dismissed", async () => {
    const confirmed = vi.spyOn(window, "confirm").mockReturnValue(false);
    await act(async () => {
      root.render(<MemoryView />);
      await Promise.resolve();
    });
    await act(async () => segment("보관됨")?.click());
    await act(async () => button("영구 삭제")?.click());

    expect(confirmed).toHaveBeenCalled();
    expect(mocks.memoryPurge).not.toHaveBeenCalled();
    confirmed.mockRestore();
  });
});

describe("MemoryView host identity", () => {
  it("clears the initial scope after a host switch", async () => {
    mocks.memoryList.mockImplementation((host: string) =>
      Promise.resolve(
        host === "local"
          ? [
              { ...memory(1, "verified", "local scoped memory"), scope_key: "localrepo" },
              { ...memory(2, "verified", "outside initial scope"), scope_key: "otherrepo" },
            ]
          : [{ ...memory(3, "verified", "remote memory"), scope_key: "remoterepo" }],
      ),
    );

    await act(async () => {
      root.render(
        <HostScopeProvider value="local">
          <MemoryView initialScope="localrepo" />
        </HostScopeProvider>,
      );
      await Promise.resolve();
    });

    expect(container.textContent).toContain("local scoped memory");
    expect(container.textContent).not.toContain("outside initial scope");

    await act(async () => {
      root.render(
        <HostScopeProvider value="remote">
          <MemoryView initialScope="localrepo" />
        </HostScopeProvider>,
      );
      await Promise.resolve();
    });

    expect(container.textContent).toContain("remote memory");
  });

  it("shows pending context and discards its response after a host switch", async () => {
    const pending = deferred<unknown>();
    mocks.memoryList.mockResolvedValue([memory(1, "verified", "memory")]);
    mocks.memoryUsages.mockResolvedValue([
      { task_id: 7, instruction: "old task", state: "done", injected_at: 1, outcome: null },
    ]);
    mocks.contextReport.mockReturnValue(pending.promise);
    await act(async () => root.render(
      <HostScopeProvider value="local"><MemoryView /></HostScopeProvider>,
    ));
    await act(async () => button("사용이력")?.click());
    await act(async () => findButtonContaining("old task")?.click());
    expect(container.textContent).toContain("작업 컨텍스트를 불러오는 중…");
    await act(async () => root.render(
      <HostScopeProvider value="remote"><MemoryView /></HostScopeProvider>,
    ));
    await act(async () => pending.resolve({
      vendors: [], injected: [], capture_enabled: false, memory_count: 0,
    }));
    expect(container.textContent).not.toContain("컨텍스트 확인");
    expect(container.textContent).not.toContain("작업 컨텍스트를 불러오는 중…");
  });

  it("does not reuse a local usage row for a remote host with colliding IDs", async () => {
    const remoteList = deferred<Memory[]>();
    const localUsage = deferred<{
      task_id: number;
      instruction: string;
      state: string;
      injected_at: number;
      outcome: null;
    }[]>();
    mocks.memoryList.mockImplementation((host: string) =>
      host === "local"
        ? Promise.resolve([memory(1, "verified", "local memory")])
        : remoteList.promise,
    );
    mocks.memoryUsages.mockImplementation((host: string) =>
      host === "local"
        ? localUsage.promise
        : Promise.resolve([
            {
              task_id: 7,
              instruction: "remote task",
              state: "done",
              injected_at: 1,
              outcome: null,
            },
          ]),
    );

    await act(async () => {
      root.render(
        <HostScopeProvider value="local">
          <MemoryView />
        </HostScopeProvider>,
      );
      await Promise.resolve();
    });
    await act(async () => button("사용이력")?.click());
    await act(async () => {
      root.render(
        <HostScopeProvider value="remote">
          <MemoryView />
        </HostScopeProvider>,
      );
    });
    await act(async () => localUsage.resolve([
      {
        task_id: 7,
        instruction: "local task",
        state: "done",
        injected_at: 1,
        outcome: null,
      },
    ]));

    expect(container.textContent).not.toContain("local task");

    await act(async () => remoteList.resolve([memory(1, "verified", "remote memory")]));
    await act(async () => button("사용이력")?.click());
    await act(async () => findButtonContaining("remote task")?.click());

    expect(mocks.contextReport).toHaveBeenCalledWith({ host: "remote", id: 7 });
  });

  it("shows the current context read error", async () => {
    mocks.memoryList.mockResolvedValue([memory(1, "verified", "memory")]);
    mocks.memoryUsages.mockResolvedValue([
      { task_id: 7, instruction: "task", state: "done", injected_at: 1, outcome: null },
    ]);
    mocks.contextReport.mockRejectedValue(new Error("unavailable"));

    await act(async () => {
      root.render(<MemoryView />);
      await Promise.resolve();
    });
    await act(async () => button("사용이력")?.click());
    await act(async () => findButtonContaining("task")?.click());

    expect(container.textContent).toContain("작업 컨텍스트를 읽지 못했습니다: Error: unavailable");
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

function findButtonContaining(text: string): HTMLButtonElement | undefined {
  return Array.from(container.querySelectorAll("button")).find((item) =>
    item.textContent?.includes(text),
  );
}
