// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const mocks = vi.hoisted(() => ({ listen: vi.fn(), state: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("../lib/ipc", () => ({ designmodeState: mocks.state }));
import { usePreviewActivation } from "./use-preview-activation";
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let container: HTMLDivElement;
const selectTask = vi.fn(() => true);
const showPreview = vi.fn();
let handler: (event: { payload: { taskId: number; generation: number } }) => void;
function Host({ selected }: { selected: number | null }) {
  usePreviewActivation({ selectedTaskId: selected, selectTask, showPreview });
  return null;
}
async function render(selected: number | null) {
  await act(async () => root.render(<Host selected={selected} />));
}
async function activate(taskId = 7, generation = 2) {
  await act(async () => handler({ payload: { taskId, generation } }));
}
beforeEach(() => {
  vi.clearAllMocks();
  container = document.createElement("div");
  root = createRoot(container);
  mocks.listen.mockImplementation(async (_name, callback) => { handler = callback; return () => undefined; });
  mocks.state.mockResolvedValue({ taskId: 7, mode: "inline", generation: 2 });
  selectTask.mockReturnValue(true);
});
afterEach(async () => { await act(async () => root.unmount()); });
it("selects the owning task before changing its panel", async () => {
  await render(3);
  await activate();
  expect(selectTask).toHaveBeenCalledWith(7);
  expect(showPreview).not.toHaveBeenCalled();
  await render(7);
  expect(showPreview).toHaveBeenCalledOnce();
});
it("reveals the current task even when task selection does not change", async () => {
  await render(7);
  await activate();
  expect(showPreview).toHaveBeenCalledOnce();
});
it("does not switch tasks for a separate window, closed surface, or stale event", async () => {
  await render(3);
  mocks.state.mockResolvedValueOnce({ taskId: 7, mode: "window", generation: 2 });
  await activate();
  mocks.state.mockResolvedValueOnce(null);
  await activate();
  await activate(7, 1);
  expect(selectTask).not.toHaveBeenCalled();
  expect(showPreview).not.toHaveBeenCalled();
});
it("does not reveal a task outside the local task list", async () => {
  selectTask.mockReturnValue(false);
  await render(7);
  await activate();
  expect(showPreview).not.toHaveBeenCalled();
});
