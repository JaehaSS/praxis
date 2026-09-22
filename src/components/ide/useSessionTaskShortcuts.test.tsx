// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../../lib/ipc";
import { SessionTaskNavigation } from "./SessionTaskNavigation";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ALPHA = "/workspace/alpha";
const BETA = "/workspace/beta";

const makeTask = (id: number, repo: string, createdAt: number): Task => ({
  id,
  host: "local",
  repo,
  branch: `feature/task-${id}`,
  base: "main",
  worktree_path: `${repo}/.praxis/worktrees/task-${id}`,
  instruction: `작업 ${id}`,
  state: "Running",
  created_at: createdAt,
  updated_at: createdAt,
  mode: "conversation",
});

// alpha가 더 최근에 쓰였으므로 목록은 alpha(1, 2) → beta(3) 순으로 그려진다.
const TASKS: Task[] = [
  makeTask(11, ALPHA, 20),
  makeTask(12, ALPHA, 30),
  makeTask(13, BETA, 5),
];

let container: HTMLDivElement | null = null;
let root: Root | null = null;
let onOpenTask: ReturnType<typeof vi.fn>;
let onDeleteTask: ReturnType<typeof vi.fn>;

const render = async (selectedKey: string | null = null): Promise<void> => {
  await act(async () => {
    root?.render(
      <SessionTaskNavigation
        tasks={TASKS}
        selectedKey={selectedKey}
        projects={[ALPHA, BETA]}
        onOpenTask={onOpenTask}
        onNewInRepo={() => {}}
        onDeleteTask={onDeleteTask}
        onRemoveProject={() => {}}
        onDiscardOrphans={() => {}}
      />,
    );
  });
};

const press = async (init: KeyboardEventInit): Promise<void> => {
  await act(async () => {
    window.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init }));
  });
};

const shortcutBadges = (): string[] =>
  [...(container?.querySelectorAll("[data-shortcut]") ?? [])].map(
    (node) => node.getAttribute("data-shortcut") ?? "",
  );

const holdMeta = async (): Promise<void> => {
  await press({ key: "Meta", metaKey: true });
  await act(async () => {
    vi.advanceTimersByTime(400);
  });
};

beforeEach(() => {
  vi.useFakeTimers();
  onOpenTask = vi.fn();
  onDeleteTask = vi.fn();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  container = null;
  root = null;
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("세션 목록 단축키", () => {
  it("⌘를 잠깐 스쳐서는 번호가 뜨지 않고, 붙잡고 있어야 목록 순서대로 붙는다", async () => {
    await render();
    expect(shortcutBadges()).toEqual([]);

    await press({ key: "Meta", metaKey: true });
    await act(async () => {
      vi.advanceTimersByTime(100); // ⌘K처럼 스쳐 지나가는 조합의 체류 시간
    });
    expect(shortcutBadges()).toEqual([]);

    await act(async () => {
      vi.advanceTimersByTime(300);
    });
    expect(shortcutBadges()).toEqual(["1", "2", "3"]);
  });

  it("⌘를 놓으면 번호를 걷는다", async () => {
    await render();
    await holdMeta();
    expect(shortcutBadges()).toHaveLength(3);

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keyup", { key: "Meta" }));
    });
    expect(shortcutBadges()).toEqual([]);
  });

  it("⌘Tab으로 창을 떠나면 번호가 남지 않는다", async () => {
    await render();
    await holdMeta();

    await act(async () => {
      window.dispatchEvent(new Event("blur"));
    });
    expect(shortcutBadges()).toEqual([]);
  });

  it("⌘+숫자는 화면에 보이는 순서의 그 작업을 연다", async () => {
    await render();
    await press({ key: "2", code: "Digit2", metaKey: true });

    // id가 아니라 작업 자체를 넘긴다 — 호스트가 다른 동명 작업과 구분되어야 한다.
    expect(onOpenTask).toHaveBeenCalledWith(TASKS[1]);
  });

  it("비어 있는 번호는 아무 일도 하지 않는다", async () => {
    await render();
    await press({ key: "9", code: "Digit9", metaKey: true });

    expect(onOpenTask).not.toHaveBeenCalled();
  });

  it("Delete는 열려 있는 작업을 확인 대화상자 없이 넘긴다 — 되돌리기는 유예 창이 맡는다", async () => {
    const confirmed = vi.spyOn(window, "confirm").mockReturnValue(true);
    await render("local:12");
    await press({ key: "Backspace" });

    expect(confirmed).not.toHaveBeenCalled();
    expect(onDeleteTask).toHaveBeenCalledWith(TASKS[1]);
  });

  it("입력 중일 때는 Delete를 가로채지 않는다", async () => {
    await render("local:12");
    const input = document.createElement("textarea");
    document.body.appendChild(input);
    input.focus();

    await press({ key: "Backspace" });

    expect(onDeleteTask).not.toHaveBeenCalled();
    input.remove();
  });

  it("열린 작업이 없으면 Delete는 무시된다", async () => {
    await render(null);
    await press({ key: "Backspace" });

    expect(onDeleteTask).not.toHaveBeenCalled();
  });
});
