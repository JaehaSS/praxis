// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { WikiPage } from "../../lib/wiki-workspace-ipc";
const mock = vi.hoisted(() => ({ create: vi.fn() }));
vi.mock("3d-force-graph", () => ({ default: mock.create }));
vi.mock("three-spritetext", async () => {
  const { Sprite } = await import("three");
  return { default: class extends Sprite { text = ""; color = ""; } };
});
import { folderGroups } from "../../lib/wiki-folder-groups";
import { createWiki3D, type Wiki3DView } from "./wiki-graph-3d";

const page = (id: string, title = id): WikiPage => ({ id, title, path: id, body: "private body", sha256: "private hash", aliases: [], tags: [], type: "page", status: "", scope: "", source_prefix: "", outgoing: [], backlinks: [] });
let container: HTMLDivElement, view: Wiki3DView | undefined;
// A mutable simulator double deliberately rewrites input nodes/links like the library.
let graph: Record<string, ReturnType<typeof vi.fn>>;
// nodeThreeObject가 만든 별을 잡아 둔다 — 색은 재질에만 남아 그래프 쪽에서는 보이지 않는다.
let stars: Map<string, any>;
let properties: Record<string, any>;
let resize: ResizeObserverCallback, intersect: IntersectionObserverCallback;
let disconnectResize: ReturnType<typeof vi.fn>, disconnectIntersection: ReturnType<typeof vi.fn>;
let failure: ReturnType<typeof vi.fn>, select: ReturnType<typeof vi.fn>;
let hidden: boolean, reduced: boolean;
let renderer: { info: { render: { frame: number } }; setPixelRatio: ReturnType<typeof vi.fn>; forceContextLoss: ReturnType<typeof vi.fn>; dispose: ReturnType<typeof vi.fn> };

beforeEach(() => {
  vi.useFakeTimers(); hidden = false; reduced = false;
  container = document.createElement("div"); document.body.append(container);
  vi.spyOn(container, "getBoundingClientRect").mockReturnValue({ width: 680, height: 320 } as DOMRect);
  vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
  vi.stubGlobal("matchMedia", () => ({ get matches() { return reduced; } }));
  disconnectResize = vi.fn(); disconnectIntersection = vi.fn();
  vi.stubGlobal("ResizeObserver", class { constructor(callback: ResizeObserverCallback) { resize = callback; } observe() {} disconnect = disconnectResize; });
  vi.stubGlobal("IntersectionObserver", class { constructor(callback: IntersectionObserverCallback) { intersect = callback; } observe() {} disconnect = disconnectIntersection; });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ createRadialGradient: () => ({ addColorStop: vi.fn() }), fillRect: vi.fn() } as unknown as CanvasRenderingContext2D);
  properties = { width: 100, height: 100, cameraPosition: { x: 0, y: 0, z: 200 }, graphData: { nodes: [], links: [] } };
  renderer = { info: { render: { frame: 0 } }, setPixelRatio: vi.fn(), forceContextLoss: vi.fn(), dispose: vi.fn() };
  stars = new Map();
  graph = {};
  const chain = ["width", "height", "backgroundColor", "showNavInfo", "enableNodeDrag", "nodeLabel", "nodeThreeObject", "linkWidth", "linkOpacity", "linkDirectionalArrowLength", "linkDirectionalArrowRelPos", "linkLabel", "onNodeClick", "onNodeHover", "warmupTicks", "cooldownTicks", "onEngineTick", "onEngineStop", "linkColor", "cameraPosition", "zoomToFit", "pauseAnimation", "resumeAnimation", "_destructor"];
  for (const name of chain) graph[name] = vi.fn((...args: any[]) => { if (!args.length && ["width", "height", "cameraPosition"].includes(name)) return properties[name]; if (args.length) properties[name] = args[0]; return graph; });
  graph.renderer = vi.fn(() => renderer);
  graph.camera = vi.fn(() => ({ fov: 50 }));
  graph.controls = vi.fn(() => ({ target: { x: 0, y: 0, z: 0 }, dispose: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  graph.postProcessingComposer = vi.fn(() => ({ dispose: vi.fn() }));
  graph.graphData = vi.fn((data?: any) => {
    if (!data) return properties.graphData;
    properties.graphData = data;
    data.nodes.forEach((node: any, i: number) => { node.x ??= i * 20; node.y ??= 0; node.z ??= 0; stars.set(node.id, properties.nodeThreeObject(node)); });
    data.links.forEach((link: any) => { link.source = data.nodes.find((node: any) => node.id === link.source); link.target = data.nodes.find((node: any) => node.id === link.target); });
    return graph;
  });
  mock.create.mockReset().mockImplementation((_container, options) => { container.append(options.rendererConfig.canvas); return graph; });
  select = vi.fn(); failure = vi.fn();
});
afterEach(() => { view?.dispose(); view = undefined; container.remove(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const start = () => { view = createWiki3D(container, select, failure); return view; };

it("isolates simulator mutations and skips rebuilding the scene for selection-only updates", () => {
  const pages = [Object.freeze(page("a")), Object.freeze(page("b"))];
  const edges = [Object.freeze({ source: "a", target: "b", evidence: [] }), { source: "a", target: "outside", evidence: [] }];
  start().update(pages, edges, null, []);
  expect(properties.graphData.nodes[0]).not.toHaveProperty("body");
  expect(properties.graphData.nodes[0]).not.toHaveProperty("sha256");
  expect(pages[0]).not.toHaveProperty("x"); expect(edges[0].source).toBe("a");
  expect(properties.graphData.links).toHaveLength(1);
  const node = properties.graphData.nodes[0];
  view!.update([...pages], edges, "b", []);
  expect(graph.graphData).toHaveBeenCalledTimes(1);
  expect(properties.graphData.nodes[0]).toBe(node);
  expect(graph.cameraPosition.mock.lastCall?.[1]).toMatchObject({ id: "b" });
  properties.onNodeClick(node); expect(select).toHaveBeenCalledWith("a");
});

it("renders labels as text, updates titles, filters endpoints, and retains surviving coordinates", () => {
  const title = '<img src=x onerror="alert(1)">';
  start().update([page("a", title), page("b")], [{ source: "a", target: "b", evidence: [] }], null, []);
  const safe = properties.nodeLabel(properties.graphData.nodes[0]);
  expect(safe.textContent).toBe(title); expect(safe.querySelector("img")).toBeNull();
  const edgeLabel = properties.linkLabel(properties.graphData.links[0]);
  expect(edgeLabel.textContent).toContain(title); expect(edgeLabel.children).toHaveLength(0);
  properties.graphData.nodes[0].x = 123;
  view!.update([page("a", "new title")], [{ source: "a", target: "b", evidence: [] }], "a", []);
  expect(properties.graphData.nodes[0]).toMatchObject({ x: 123, title: "new title" });
  expect(properties.graphData.links).toHaveLength(0);
});

it("honors reduced motion for selection, zoom and framing and ignores unchanged resize notifications", () => {
  reduced = true;
  start().update([page("a"), page("b")], [], null, []);
  view!.update([page("a"), page("b")], [], "a", []);
  expect(graph.cameraPosition.mock.lastCall?.[2]).toBe(0);
  view!.zoom(0.8); expect(graph.cameraPosition.mock.lastCall?.[2]).toBe(0);
  view!.reset(); expect(graph.zoomToFit).toHaveBeenLastCalledWith(0, 36);
  graph.zoomToFit.mockClear();
  resize([], {} as ResizeObserver); expect(graph.zoomToFit).not.toHaveBeenCalled();
});

it("pauses rendering and the watchdog while hidden or offscreen, then resumes", () => {
  start();
  hidden = true; document.dispatchEvent(new Event("visibilitychange"));
  expect(graph.pauseAnimation).toHaveBeenCalled();
  vi.advanceTimersByTime(10_000); expect(failure).not.toHaveBeenCalled();
  hidden = false; document.dispatchEvent(new Event("visibilitychange"));
  expect(graph.resumeAnimation).toHaveBeenCalledTimes(2);
  intersect([{ isIntersecting: false } as IntersectionObserverEntry], {} as IntersectionObserver);
  vi.advanceTimersByTime(10_000); expect(failure).not.toHaveBeenCalled();
  intersect([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver);
  expect(graph.resumeAnimation).toHaveBeenCalledTimes(3);
});

it("recovers once after context loss and releases resources even when destruction throws", () => {
  start().update([page("a")], [], null, []);
  graph._destructor.mockImplementation(() => { throw new Error("broken renderer"); });
  container.querySelector("canvas")!.dispatchEvent(new Event("webglcontextlost"));
  view!.dispose();
  expect(failure).toHaveBeenCalledOnce(); expect(renderer.dispose).toHaveBeenCalledOnce();
  expect(disconnectResize).toHaveBeenCalledOnce(); expect(disconnectIntersection).toHaveBeenCalledOnce();
  expect(vi.getTimerCount()).toBe(0); expect(container.children).toHaveLength(0);
});

it("recovers from a stopped render loop or invalid camera without waiting forever", () => {
  start(); vi.advanceTimersByTime(4_000);
  expect(failure).toHaveBeenCalledOnce(); expect(graph._destructor).toHaveBeenCalledOnce();
});

it("does not report failure after a normal unmount and propagates constructor errors for the React fallback", () => {
  start(); view!.dispose(); vi.advanceTimersByTime(10_000);
  expect(failure).not.toHaveBeenCalled(); expect(graph._destructor).toHaveBeenCalledOnce();
  mock.create.mockImplementation(() => { throw new Error("WebGL unavailable"); });
  expect(() => createWiki3D(container, select, failure)).toThrow("WebGL unavailable");
  expect(container.children).toHaveLength(0);
});

it("colours a star by its folder, folds unranked folders into neutral ink, and lets selection win", () => {
  const groups = folderGroups(["연구/a.md", "연구/w.md", "연구/x.md", "일지/b.md", "일지/y.md", "메모/c.md", "잡동/d.md"]);
  const pages = ["연구/a.md", "일지/b.md", "메모/c.md", "잡동/d.md"].map(id => page(id));
  const color = (id: string) => stars.get(id).children[0].material.color.getHexString();

  start().update(pages, [], null, groups);

  // 테마 변수가 없는 환경이라 모듈 기본값이 나온다 — 확인하려는 것은 값이 아니라 어느 문서가 어느 슬롯을 받느냐다.
  expect([color("연구/a.md"), color("일지/b.md"), color("메모/c.md")]).toEqual(["3987e5", "d95926", "199e70"]);
  expect(color("잡동/d.md")).toBe("a1a1aa");

  view!.update(pages, [], "일지/b.md", groups);

  expect(color("일지/b.md")).toBe("14b8a6");
  expect(color("연구/a.md")).toBe("3987e5");
});

it("keeps folder colours when the selection changes and does not rebuild the scene for them", () => {
  const groups = folderGroups(["연구/a.md", "일지/b.md"]);
  const pages = [page("연구/a.md"), page("일지/b.md")];
  start().update(pages, [], null, groups);
  const built = graph.graphData.mock.calls.length;

  view!.update(pages, [], "연구/a.md", groups);

  expect(graph.graphData.mock.calls.length).toBe(built);
  expect(stars.get("일지/b.md").children[0].material.color.getHexString()).toBe("d95926");
});
