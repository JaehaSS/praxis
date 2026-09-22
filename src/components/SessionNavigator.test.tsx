// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../lib/ipc";
import type { ProjectGroups } from "../lib/project-groups";
import { SessionNavigator } from "./SessionNavigator";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const repo = "/workspace/alpha";
const nextRepo = "/workspace/beta";
const groups: ProjectGroups = {
  version: 1,
  groups: [{ id: "team", name: "Team", collapsed: true }],
  assignment: { [repo]: "team" },
};
const task = (id: number, name: string, target = repo): Task => ({
  id,
  host: "local",
  repo: target,
  branch: `branch-${id}`,
  base: "main",
  worktree_path: target,
  instruction: name,
  state: "Running",
  created_at: id,
  updated_at: id,
  mode: "conversation",
});

let host: HTMLDivElement;
let root: Root;
const onClose = vi.fn();
const onOpenTask = vi.fn();
const scrollIntoView = vi.fn();

const render = async (tasks = [task(1, "first"), task(2, "second")]) => {
  await act(async () => {
    root.render(<SessionNavigator open tasks={tasks} projects={[repo, nextRepo]} groups={groups} onClose={onClose} onOpenTask={onOpenTask} />);
  });
};

const input = (): HTMLInputElement => host.querySelector("input")!;
const press = async (key: string) => {
  await act(async () => input().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true })));
};
const row = (label: string): HTMLButtonElement =>
  [...host.querySelectorAll<HTMLButtonElement>('[role="treeitem"]')].find((item) =>
    item.textContent?.includes(label),
  )!;
const pressRow = async (label: string, key: string) => {
  await act(async () => {
    row(label).focus();
    row(label).dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
};
const search = async (value: string) => {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    setter?.call(input(), value);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
};

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  onClose.mockClear();
  onOpenTask.mockClear();
  scrollIntoView.mockClear();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: scrollIntoView });
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  delete (HTMLElement.prototype as { scrollIntoView?: unknown }).scrollIntoView;
});

describe("SessionNavigator", () => {
  it("keeps keyboard order in visible tree order and opens the selected session", async () => {
    await render();

    await act(async () => row("Team").dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await pressRow("Team", "ArrowRight");
    await pressRow("alpha", "ArrowRight");
    await pressRow("first", "Enter");

    expect(onOpenTask).toHaveBeenCalledWith(expect.objectContaining({ id: 1, host: "local" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("reveals collapsed search paths and labels them as expanded", async () => {
    await render();
    await search("second");
    expect(host.textContent).toContain("Team");
    expect(host.textContent).toContain("alpha");
    expect(host.textContent).toContain("second");
    expect(row("Team").getAttribute("aria-expanded")).toBe("true");
    expect(row("Team").getAttribute("tabindex")).toBe("0");

    await search("");
    expect(host.textContent).toContain("미소속 프로젝트");
  });

  it("closes an empty search from the input and offers a single tree tab stop", async () => {
    await render();
    expect(host.textContent).toContain("트리에서 ←→");
    expect(host.querySelector('[aria-label="세션 탐색 닫기"]')).not.toBeNull();
    expect(host.querySelectorAll('[role="treeitem"][tabindex="0"]')).toHaveLength(1);
    await search("missing");
    await press("Escape");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes from a focused tree row", async () => {
    await render();
    await pressRow("Team", "Escape");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
