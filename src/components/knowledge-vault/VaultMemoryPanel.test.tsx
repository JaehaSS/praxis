// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const mocks = vi.hoisted(() => ({ files: vi.fn(), settings: vi.fn(), save: vi.fn(), open: vi.fn(), session: vi.fn() }));
vi.mock("../../lib/memory-file-ipc", () => ({ memoryFilesList: mocks.files, memorySettingsGet: mocks.settings, memorySettingsSet: mocks.save, memoryFileOpen: mocks.open, memorySessionOpen: mocks.session }));
import { VaultMemoryPanel } from "./VaultMemoryPanel";

const SETTINGS = { root: "", effective_root: "/vault/memory", cap_lines: 100, cap_bytes: 8192, user_cap_lines: 40, user_cap_bytes: 3072 };
const user = { path: "/vault/memory/USER.md", kind: "user" as const, repo: null, repo_key: null, exists: false, lines: 0, bytes: 0, cap_lines: 40, cap_bytes: 3072, modified_at: null, last_projected_at: null, last_task_id: null };
const repo = { path: "/vault/memory/praxis-3f9a1c2e/MEMORY.md", kind: "repo" as const, repo: "/Users/me/praxis", repo_key: "praxis-3f9a1c2e", exists: true, lines: 42, bytes: 1200, cap_lines: 100, cap_bytes: 8192, modified_at: 1_757_700_000, last_projected_at: 1_757_700_500, last_task_id: 812 };

let node: HTMLDivElement; let root: Root;
beforeEach(() => {
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.settings.mockResolvedValue(SETTINGS); mocks.files.mockResolvedValue([user, repo]); mocks.open.mockResolvedValue(undefined); mocks.session.mockResolvedValue({ label: "memory", root: "/vault/memory", skill_present: true, warning: null });
  node = document.createElement("div"); document.body.appendChild(node); root = createRoot(node);
});
afterEach(() => { act(() => root.unmount()); node.remove(); });

const button = (label: string) => Array.from(node.querySelectorAll("button")).find(item => item.textContent === label) as HTMLButtonElement;
const render = async () => { await act(async () => { root.render(<VaultMemoryPanel host="local" />); }); await act(async () => { await Promise.resolve(); await Promise.resolve(); }); };

it("shows every memory file including one that does not exist yet", async () => {
  await render();
  expect(node.textContent).toContain("전역 USER.md");
  expect(node.textContent).toContain("praxis");
  expect(node.textContent).toContain("/vault/memory/praxis-3f9a1c2e/MEMORY.md");
  expect(node.textContent).toContain("42 / 100줄");
  expect(node.textContent).toContain("#812");
  expect(button("만들기")).toBeTruthy();
  expect(button("열기")).toBeTruthy();
});

it("warns when a file passed its line cap", async () => {
  mocks.files.mockResolvedValue([{ ...repo, lines: 140 }]);
  await render();
  expect(node.textContent).toContain("상한 초과 — 넘긴 줄은 실리지 않는다");
});

it("shows the root and what memory is when no file exists", async () => {
  mocks.files.mockResolvedValue([]);
  await render();
  expect(node.textContent).toContain("/vault/memory");
  expect(node.textContent).toContain("에이전트가 작업 중 직접 쓴다. 작업 시작 때 AGENTS.md 블록으로 실린다.");
});

it("opens the file the row points at", async () => {
  await render();
  await act(async () => { button("열기").click(); });
  expect(mocks.open).toHaveBeenCalledWith("/vault/memory/praxis-3f9a1c2e/MEMORY.md", "local");
});

it("opens a cleanup session with the chosen agent", async () => {
  await act(async () => { root.render(<VaultMemoryPanel agent="codex" host="local" />); });
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  await act(async () => { button("정리 세션 열기").click(); });
  expect(mocks.session).toHaveBeenCalledWith("codex", "local");
});

it("saves changed caps and reloads the list", async () => {
  mocks.save.mockResolvedValue({ ...SETTINGS, cap_lines: 120 });
  await render();
  const input = node.querySelector("input[aria-label='MEMORY.md 줄 상한']") as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!.call(input, "120");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => { button("저장").click(); });
  expect(mocks.save).toHaveBeenCalledWith({ ...SETTINGS, cap_lines: 120 }, "local");
  expect(mocks.files).toHaveBeenCalledTimes(2);
});

it("does not call local IPC for a remote host", async () => {
  await act(async () => { root.render(<VaultMemoryPanel host="remote" />); });
  expect(node.textContent).toContain("로컬 세션에서만");
  expect(mocks.files).not.toHaveBeenCalled();
});
