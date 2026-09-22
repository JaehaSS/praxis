// @vitest-environment jsdom

import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../../lib/ipc";
import type { ProjectGroups } from "../../lib/project-groups";
import { Sidebar } from "./Sidebar";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ALPHA = "/workspace/alpha";
const BETA = "/workspace/beta";

const task = (id: number, repo: string, instruction: string): Task => ({
  id,
  host: "local",
  repo,
  branch: `feature/task-${id}`,
  base: "main",
  worktree_path: `${repo}/.praxis/worktrees/task-${id}`,
  instruction,
  state: "Running",
  created_at: id,
  updated_at: id,
  mode: "conversation",
});

const TASKS = [task(1, ALPHA, "alpha task"), task(2, BETA, "beta task")];
const COLLAPSED_PROJECTS_STORAGE_KEY = "praxis-desktop-collapsed-projects";

interface SidebarHarnessProps {
  tasks?: Task[];
  projects?: string[];
  groups?: ProjectGroups;
  onNewInRepo?: (repo: string) => void;
}

function SidebarHarness({
  tasks = TASKS,
  projects = [ALPHA, BETA],
  groups,
  onNewInRepo = () => {},
}: SidebarHarnessProps) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <Sidebar
      browsingHost="local"
      onPickBrowsingHost={() => {}}
      view="home"
      onNewTask={() => {}}
      onQuickLink={() => {}}
      collapsed={collapsed}
      onToggleCollapse={() => setCollapsed((current) => !current)}
      tasks={tasks}
      selectedKey={null}
      projects={projects}
      onOpenTask={() => {}}
      onNewInRepo={onNewInRepo}
      onDeleteTask={() => {}}
      onRemoveProject={() => {}}
      onDiscardOrphans={() => {}}
      groups={groups}
    />
  );
}

let container: HTMLDivElement;
let root: Root;

const projectHeader = (repo: string): HTMLElement =>
  Array.from(container.querySelectorAll<HTMLElement>('[draggable="true"]')).find((header) =>
    header.textContent?.includes(repo.split("/").pop() ?? ""),
  )!;

/** 실제 포인터가 내는 순서 — 메뉴는 mousedown에서 닫히고, 이어지는 click이 헤더에 닿는다. */
const pressThrough = async (element: HTMLElement) => {
  await act(async () => {
    for (const type of ["mousedown", "mouseup", "click"]) {
      element.dispatchEvent(new MouseEvent(type, { bubbles: true }));
    }
  });
};

const click = async (element: HTMLElement) => {
  await act(async () => {
    element.click();
  });
};

const render = async (props: SidebarHarnessProps = {}) => {
  await act(async () => root.render(<SidebarHarness {...props} />));
};

beforeEach(() => {
  // 프로젝트 컨텍스트 메뉴가 쓴다 — jsdom에는 ResizeObserver가 없다.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  localStorage.clear();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Sidebar 프로젝트 접힘", () => {
  it("전체 사이드바를 다시 열어도 닫은 프로젝트 작업은 숨긴다", async () => {
    await render();

    await click(projectHeader(ALPHA));
    expect(container.textContent).not.toContain("alpha task");
    expect(container.textContent).toContain("beta task");

    await click(container.querySelector<HTMLElement>('[aria-label="사이드바 접기"]')!);
    await click(container.querySelector<HTMLElement>('[aria-label="사이드바 펼치기"]')!);

    expect(container.textContent).not.toContain("alpha task");
    expect(container.textContent).toContain("beta task");
  });

  it("목록 순서, 그룹, 작업 상태가 갱신돼도 닫은 프로젝트를 다시 열지 않는다", async () => {
    await render();
    await click(projectHeader(ALPHA));

    await render({
      tasks: [
        task(3, BETA, "beta task"),
        { ...task(4, ALPHA, "alpha task"), state: "Queued" },
      ],
      projects: [BETA, ALPHA],
      groups: {
        version: 1,
        groups: [{ id: "group-1", name: "팀", collapsed: false }],
        assignment: { [ALPHA]: "group-1" },
      },
    });

    expect(container.textContent).not.toContain("alpha task");
    expect(container.textContent).toContain("beta task");
  });

  it("새 작업 버튼은 닫힌 프로젝트를 열지 않고 명시적 재열기는 남긴다", async () => {
    const created: string[] = [];
    await render({ onNewInRepo: (repo) => created.push(repo) });
    await click(projectHeader(ALPHA));
    await click(projectHeader(ALPHA).querySelector<HTMLButtonElement>("button")!);

    expect(created).toEqual([ALPHA]);
    expect(container.textContent).not.toContain("alpha task");
    expect(container.textContent).toContain("beta task");

    await click(projectHeader(ALPHA));
    expect(container.textContent).toContain("alpha task");
    expect(container.textContent).toContain("beta task");

    await click(container.querySelector<HTMLElement>('[aria-label="사이드바 접기"]')!);
    await click(container.querySelector<HTMLElement>('[aria-label="사이드바 펼치기"]')!);
    expect(container.textContent).toContain("alpha task");
    expect(container.textContent).toContain("beta task");
  });

  it("컨텍스트 메뉴를 닫으려고 다른 프로젝트 헤더를 누르면 그 프로젝트가 펼쳐지지 않는다", async () => {
    await render();
    await click(projectHeader(ALPHA));
    expect(container.textContent).not.toContain("alpha task");

    await act(async () => {
      projectHeader(BETA).dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, clientX: 10, clientY: 10 }),
      );
    });
    expect(document.querySelector('[role="menu"]')).not.toBeNull();

    await pressThrough(projectHeader(ALPHA));
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(container.textContent).not.toContain("alpha task");

    await pressThrough(projectHeader(ALPHA));
    expect(container.textContent).toContain("alpha task");
  });

  it("토글은 저장소의 현재 값 위에 쓴다", async () => {
    await render();
    localStorage.setItem(COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([BETA]));

    await click(projectHeader(ALPHA));

    expect(new Set(JSON.parse(localStorage.getItem(COLLAPSED_PROJECTS_STORAGE_KEY)!))).toEqual(
      new Set([ALPHA, BETA]),
    );
    expect(container.textContent).not.toContain("alpha task");
    expect(container.textContent).not.toContain("beta task");
  });

  it.each(["not JSON", '["/workspace/alpha", 3]'])(
    "손상된 저장값(%s)과 저장소 오류에서도 현재 세션의 토글은 동작한다",
    async (stored) => {
      localStorage.setItem(COLLAPSED_PROJECTS_STORAGE_KEY, stored);
      await render();
      expect(container.textContent).toContain("alpha task");
      await act(async () => root.unmount());
      root = createRoot(container);

      const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
        throw new Error("storage unavailable");
      });
      const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new Error("storage unavailable");
      });

      await render();
      await click(projectHeader(ALPHA));

      expect(getItem).toHaveBeenCalledWith(COLLAPSED_PROJECTS_STORAGE_KEY);
      expect(setItem).toHaveBeenCalledWith(
        COLLAPSED_PROJECTS_STORAGE_KEY,
        JSON.stringify([ALPHA]),
      );
      expect(container.textContent).not.toContain("alpha task");
    },
  );
});
