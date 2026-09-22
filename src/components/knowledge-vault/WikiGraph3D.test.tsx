// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Wiki3DView } from "./wiki-graph-3d";
import type { WikiPage } from "../../lib/wiki-workspace-ipc";
const mocks = vi.hoisted(() => ({ create: vi.fn() }));
vi.mock("./wiki-graph-3d", () => ({ createWiki3D: mocks.create }));
import { WikiGraph3D } from "./WikiGraph3D";
import { WikiGraphCanvas } from "./WikiGraphCanvas";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let container: HTMLDivElement, root: Root;
let controller: { current: Wiki3DView | null };
let view: { [K in keyof Wiki3DView]: ReturnType<typeof vi.fn> };
const onSelect = vi.fn(), onFailure = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  container = document.createElement("div"); document.body.append(container); root = createRoot(container);
  controller = { current: null };
  view = { update: vi.fn(), zoom: vi.fn(), reset: vi.fn(), dispose: vi.fn() };
  mocks.create.mockReturnValue(view);
});
afterEach(() => { act(() => root.unmount()); container.remove(); });
const render = (selected = "a", select = onSelect) => root.render(<WikiGraph3D pages={[]} edges={[]} groups={[]} selected={selected} onSelect={select} onFailure={onFailure} controller={controller} />);

it("does not create a renderer if its lazy import completes after unmount", async () => {
  act(() => render());
  act(() => root.render(null));
  await act(async () => { await vi.dynamicImportSettled(); });
  expect(mocks.create).not.toHaveBeenCalled(); expect(controller.current).toBeNull();
});

it("uses current selection/callbacks without remounting and disposes the controller on exit", async () => {
  await act(async () => { render(); await vi.dynamicImportSettled(); });
  const changed = vi.fn();
  act(() => render("b", changed));
  expect(mocks.create).toHaveBeenCalledOnce();
  expect(view.update).toHaveBeenLastCalledWith([], [], "b", []);
  mocks.create.mock.calls[0][1]("b"); expect(changed).toHaveBeenCalledWith("b"); expect(onSelect).not.toHaveBeenCalled();
  act(() => root.render(null));
  expect(view.dispose).toHaveBeenCalledOnce(); expect(controller.current).toBeNull();
  mocks.create.mock.calls[0][2](); expect(onFailure).not.toHaveBeenCalled();
  mocks.create.mock.calls[0][1]("a"); expect(changed).toHaveBeenCalledOnce();
});

it("reports a renderer startup failure for 2D fallback", async () => {
  mocks.create.mockImplementationOnce(() => { throw new Error("WebGL unavailable"); });
  await act(async () => { render(); await vi.dynamicImportSettled(); });
  expect(onFailure).toHaveBeenCalledOnce(); expect(controller.current).toBeNull();
});

it("caps both modes at 200 without reordering on visible selection and includes a selected overflow document", async () => {
  const pages = Array.from({ length: 201 }, (_, i) => ({ id: String(i), path: `${i}.md`, title: `문서 ${i}` } as WikiPage));
  const draw = (selected: string) => root.render(<WikiGraphCanvas pages={pages} edges={[]} groups={[]} selected={selected} onSelect={onSelect} />);
  await act(async () => { draw("0"); await vi.dynamicImportSettled(); });
  const initial = view.update.mock.lastCall![0].map((p: WikiPage) => p.id);
  act(() => draw("150"));
  expect(view.update.mock.lastCall![0].map((p: WikiPage) => p.id)).toEqual(initial);
  act(() => draw("200"));
  expect(view.update.mock.lastCall![0]).toHaveLength(200);
  expect(view.update.mock.lastCall![0].at(-1).id).toBe("200");
  act(() => [...container.querySelectorAll("button")].find(b => b.textContent === "2D")!.click());
  expect(container.querySelectorAll('g[role="button"]')).toHaveLength(200);
  expect(container.querySelector('g[aria-pressed="true"]')?.getAttribute("aria-label")).toBe("그래프 문서: 문서 200");
});
