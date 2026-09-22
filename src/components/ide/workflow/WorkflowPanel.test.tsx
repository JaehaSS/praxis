// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../../../lib/ipc";
import { emptyProgressView, progressStorageKey } from "../../../lib/workflow/progress";
import { WorkflowPanel } from "./WorkflowPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let container: HTMLDivElement; let root: Root;
const openTask = vi.fn();
const refresh = vi.fn();
const task = (id: number, extra: Partial<Task> = {}): Task => ({
  id, host: "local", repo: "/repo", branch: `task-${id}`, base: "main", worktree_path: `/work/${id}`,
  instruction: `작업 ${id}`, state: "Queued", created_at: id, updated_at: id, mode: "conversation", ...extra,
});
const render = (tasks: Task[], host = "local") => act(() => {
  root.render(<WorkflowPanel tasks={tasks} host={host} onOpenTask={openTask} onRefresh={refresh} />);
});
const click = (text: string) => act(() => {
  const button = [...container.querySelectorAll("button")].find((element) => element.textContent === text);
  if (!button) throw new Error(`Missing button: ${text}`);
  button.click();
});
const select = (label: string, value: string) => act(() => {
  const element = container.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!;
  element.value = value;
  element.dispatchEvent(new Event("change", { bubbles: true }));
});
const node = (id: number) => container.querySelector<HTMLButtonElement>(`button[aria-label^="작업 #${id}:"]`)!;
const summary = () => container.querySelector('[aria-label="작업 진행 요약"]')!.textContent;

beforeEach(() => {
  localStorage.clear(); vi.clearAllMocks();
  container = document.createElement("div"); document.body.appendChild(container); root = createRoot(container);
});
afterEach(() => { act(() => root.unmount()); container.remove(); vi.restoreAllMocks(); });

describe("local development progress view", () => {
  it("shows current tasks with no execution setup and opens the selected existing task", () => {
    const tasks = [task(1, { state: "Running" }), task(2, { state: "AwaitingReview" }), task(3, { state: "Done" })];
    render(tasks);
    expect(node(1).textContent).toContain("실행 중");
    expect(node(2).textContent).toContain("검토 대기");
    expect(summary()).toContain("완료 1");
    expect(container.textContent).not.toMatch(/Runner|Podman|JSON|설치/);
    act(() => node(2).click()); click("작업 열기");
    expect(openTask).toHaveBeenCalledWith(tasks[1]);
    click("새로고침"); expect(refresh).toHaveBeenCalledOnce();
  });

  it("follows live task updates while retaining view annotations", () => {
    render([task(1, { state: "Running" })]); select("작업 Phase", "구현");
    render([task(1, { state: "AwaitingReview", awaiting_kind: "question" })]);
    expect(node(1).textContent).toContain("답변 대기");
    expect(node(1).textContent).toContain("구현");
    expect(summary()).toContain("완료 0");
    render([task(1, { state: "Done" })]);
    expect(summary()).toContain("완료 1");
    const saved = JSON.parse(localStorage.getItem(progressStorageKey("local", "/repo"))!);
    expect(saved.phaseByTask).toEqual({ "1": "구현" });
    expect(saved).not.toHaveProperty("state");
  });

  it("persists phases and connections, rejects a cycle and removes a connection", () => {
    const tasks = [task(1), task(2)];
    render(tasks); act(() => node(2).click());
    select("작업 Phase", "검증"); select("선행 작업 선택", "1"); click("연결");
    expect(localStorage.getItem(progressStorageKey("local", "/repo"))).toContain('"from":1,"to":2');
    act(() => root.unmount()); root = createRoot(container); render(tasks);
    expect(node(2).textContent).toContain("검증");
    expect([...container.querySelectorAll('select[aria-label="선행 작업 선택"] option')].map((option) => option.getAttribute("value"))).toEqual([""]);
    act(() => node(2).click());
    act(() => container.querySelector<HTMLButtonElement>('[aria-label="선행 작업 #1 연결 해제"]')!.click());
    expect(JSON.parse(localStorage.getItem(progressStorageKey("local", "/repo"))!).dependencies).toEqual([]);
  });

  it("isolates annotations and colliding task IDs across projects and hosts", () => {
    const tasks = [task(1), task(2, { repo: "/second" }), task(1, { host: "remote", instruction: "원격 작업" })];
    render(tasks); select("작업 Phase", "계획");
    select("그래프 프로젝트", "/second");
    expect(node(1)).toBeNull(); expect(localStorage.getItem(progressStorageKey("local", "/second"))).toBeNull();
    select("작업 Phase", "검증");
    select("그래프 프로젝트", "/repo"); expect(node(1).textContent).toContain("계획");
    render(tasks, "remote"); expect(node(1).textContent).toContain("원격 작업"); expect(node(1).textContent).toContain("미분류");
    select("작업 Phase", "구현");
    render(tasks); expect(node(1).textContent).toContain("계획");
  });

  it("shows continuation as history and drops edges whose task is removed", () => {
    const first = task(1, { state: "Done" }); const next = task(2, { resumed_from: 1 });
    render([first, next]); act(() => node(2).click());
    expect(container.querySelector('[aria-label="선택한 작업 상세"]')!.textContent).toContain("이어받음");
    expect(container.querySelector("path[stroke-dasharray]")).not.toBeNull();
    render([next]);
    expect(container.querySelector("path[stroke-dasharray]")).toBeNull();
    expect(container.querySelector('[aria-label="선택한 작업 상세"]')!.textContent).toContain("지정된 선행 작업이 없습니다");
  });

  it("labels offline tasks as last known state, excludes their completion and disables opening", () => {
    render([task(1, { state: "Done", stale: true })]);
    expect(node(1).textContent).toContain("마지막 확인"); expect(summary()).toContain("완료 0");
    click("작업 열기"); expect(openTask).not.toHaveBeenCalled();
  });

  it("keeps tasks observable when stored annotations are corrupt or storage cannot be written", () => {
    localStorage.setItem(progressStorageKey("local", "/repo"), "invalid");
    render([task(1, { state: "Running" })]);
    expect(container.textContent).toContain("저장된 그래프 설정을 읽지 못했습니다");
    expect(node(1).textContent).toContain("실행 중");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("quota"); });
    select("작업 Phase", "구현");
    expect(node(1).textContent).toContain("구현");
    expect(container.textContent).toContain("그래프 설정을 저장하지 못했습니다");
  });

  it("reloads annotations changed by another window without writing over them", () => {
    render([task(1)]);
    const key = progressStorageKey("local", "/repo");
    const value = JSON.stringify({ ...emptyProgressView(), phaseByTask: { "1": "검증" } });
    localStorage.setItem(key, value);
    act(() => window.dispatchEvent(new StorageEvent("storage", { key, newValue: value })));
    expect(node(1).textContent).toContain("검증"); expect(localStorage.getItem(key)).toBe(value);
  });

  it("renders task content as text and provides an empty state", () => {
    render([task(1, { instruction: '<img src=x onerror="alert(1)">' })]);
    expect(node(1).textContent).toContain("<img"); expect(container.querySelector("img")).toBeNull();
    render([]); expect(container.textContent).toContain("작업을 만들면");
  });
});
