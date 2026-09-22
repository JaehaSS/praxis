import ForceGraph3D, { type ForceGraph3DInstance, type NodeObject, type LinkObject } from "3d-force-graph";
import { CanvasTexture, Group, Sprite, SpriteMaterial, type PerspectiveCamera, type Vector3 } from "three";
import SpriteText from "three-spritetext";
import type { WikiEdge, WikiPage } from "../../lib/wiki-workspace-ipc";
import { folderSlotOf, type FolderGroup } from "../../lib/wiki-folder-groups";

interface GraphNode extends NodeObject { id: string; title: string; }
interface GraphLink extends LinkObject<GraphNode> { source: string | GraphNode; target: string | GraphNode; }
interface Star { group: Group; sprite: Sprite; caption?: SpriteText; }
export interface Wiki3DView {
  update(pages: WikiPage[], edges: WikiEdge[], selected: string | null, groups: readonly FolderGroup[]): void;
  zoom(factor: number): void;
  reset(): void;
  dispose(): void;
}
type Point = { x: number; y: number; z: number };
const finite = (point: Partial<Point> | undefined): point is Point => !!point && [point.x, point.y, point.z].every(Number.isFinite);
const idOf = (node: string | GraphNode) => typeof node === "string" ? node : node.id;
const label = (value: string) => { const span = document.createElement("span"); span.textContent = value; return span; };

/** This module is lazy-loaded. Only renderer-owned copies enter the force simulation. */
export function createWiki3D(container: HTMLDivElement, onSelect: (id: string) => void, onFailure: () => void): Wiki3DView {
  let graph: ForceGraph3DInstance<GraphNode, GraphLink> | undefined;
  let disposed = false, intersecting = true, active = false, needsFrame = true, focusSelection = false;
  let selected: string | null = null, hovered: string | null = null, signature = "";
  let nodes = new Map<string, GraphNode>();
  // 폴더 색은 노드 바깥에 둔다. 노드에 실으면 시그니처가 바뀌어, 색만 달라져도 힘 시뮬레이션이 처음부터 다시 돈다.
  let slots = new Map<string, number | null>();
  const stars = new Map<string, Star>();
  let timer: ReturnType<typeof setInterval> | undefined;
  let resizeObserver: ResizeObserver | undefined, intersectionObserver: IntersectionObserver | undefined, themeObserver: MutationObserver | undefined;
  let lastFrame = -1, stalled = 0;
  const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const duration = () => motion.matches ? 0 : 350;
  let colors = { background: "#101014", node: "#a1a1aa", selected: "#14b8a6", link: "#71717a", categorical: ["#3987e5", "#d95926", "#199e70"] };
  const canvas = document.createElement("canvas");

  const safely = (action: () => void) => { if (!disposed) { try { action(); } catch { fail(); } } };
  const release = (action: () => void) => { try { action(); } catch { /* Continue releasing the other resources. */ } };
  function dispose() {
    if (disposed) return;
    disposed = true; active = false;
    clearInterval(timer);
    resizeObserver?.disconnect(); intersectionObserver?.disconnect(); themeObserver?.disconnect();
    document.removeEventListener("visibilitychange", visibility);
    window.removeEventListener("pagehide", pause); window.removeEventListener("pageshow", visibility);
    canvas.removeEventListener("webglcontextlost", fail);
    release(() => (graph?.controls() as { removeEventListener(type: string, callback: () => void): void } | undefined)?.removeEventListener("change", cameraChanged));
    release(() => graph?.pauseAnimation());
    release(() => graph?.renderer().forceContextLoss());
    release(() => graph?._destructor());
    release(() => (graph?.controls() as { dispose(): void } | undefined)?.dispose());
    release(() => graph?.renderer().dispose());
    release(() => graph?.postProcessingComposer().dispose());
    for (const star of stars.values()) releaseStar(star);
    stars.clear(); nodes.clear();
    container.replaceChildren();
  }
  function fail() { if (!disposed) { dispose(); onFailure(); } }
  function releaseStar(star: Star) {
    for (const sprite of [star.sprite, star.caption]) if (sprite) {
      release(() => sprite.material.map?.dispose()); release(() => sprite.material.dispose());
    }
  }
  function target() { return (graph!.controls() as { target: Vector3 }).target; }
  /** 선택은 폴더 색을 덮는다. 덮인 소속은 크기·캡션·범례가 아니라 옆의 문서 목록이 계속 알려준다. */
  function starColor(id: string) {
    if (id === selected) return colors.selected;
    const slot = slots.get(id);
    return slot === null || slot === undefined ? colors.node : colors.categorical[slot] ?? colors.node;
  }
  function paintStar(id: string, star: Star) {
    const focused = id === selected || id === hovered;
    const color = starColor(id);
    star.sprite.material.color.set(color);
    star.sprite.material.opacity = focused ? 1 : 0.9;
    star.sprite.scale.setScalar(focused ? 18 : 14);
    if (focused && !star.caption) {
      star.caption = new SpriteText("", 4);
      star.caption.position.set(0, -9, 0);
      // Labels must not steal the hit target from another document's star.
      star.caption.raycast = () => {};
      star.group.add(star.caption);
    }
    if (star.caption) {
      star.caption.visible = focused;
      const text = nodes.get(id)?.title ?? "";
      const title = text.length > 36 ? `${text.slice(0, 35)}…` : text;
      if (focused && star.caption.text !== title) star.caption.text = title;
      if (star.caption.color !== color) star.caption.color = color;
    }
  }
  function paint() {
    for (const [id, star] of stars) paintStar(id, star);
    graph!.linkColor(link => [idOf(link.source), idOf(link.target)].includes(selected ?? "") ? colors.selected : colors.link);
    sizeCaptions();
  }
  function sizeCaptions() {
    const camera = graph!.cameraPosition();
    const fov = (graph!.camera() as PerspectiveCamera).fov * Math.PI / 180;
    for (const [id, { caption, sprite }] of stars) {
      const node = nodes.get(id);
      if (!caption?.visible || !finite(node) || !finite(camera)) continue;
      // Keep the selected/hovered title near 12 screen pixels as the camera moves.
      const distance = Math.hypot(camera.x - node.x, camera.y - node.y, camera.z - node.z);
      const height = Math.max(0.1, 24 * distance * Math.tan(fov / 2) / graph!.height());
      const aspect = caption.scale.x / caption.scale.y;
      caption.scale.set(height * aspect, height, 1);
      caption.position.y = -sprite.scale.y / 2 - height;
    }
  }
  function cameraChanged() { safely(sizeCaptions); }
  function theme() {
    const css = getComputedStyle(container);
    const color = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
    colors = {
      background: color("--c-bg", colors.background),
      // 색 슬롯을 받지 못한 폴더(기타)의 노드 색이다. 링크와 같은 --c-text-muted를 쓰면 노드와 선이 한 덩어리로 보인다.
      node: color("--c-text-2", colors.node),
      selected: color("--c-primary", colors.selected),
      link: color("--c-text-muted", colors.link),
      categorical: colors.categorical.map((fallback, index) => color(`--c-cat-${index + 1}`, fallback)),
    };
    graph!.backgroundColor(colors.background);
    paint();
  }
  function frame() {
    if (!active || !graph || !nodes.size) return;
    const node = selected ? nodes.get(selected) : undefined;
    if (focusSelection && finite(node)) {
      const camera = graph.cameraPosition();
      if (!finite(camera)) throw new Error("Invalid graph camera");
      let dx = camera.x - node.x, dy = camera.y - node.y, dz = camera.z - node.z;
      const length = Math.hypot(dx, dy, dz);
      if (!length) { dx = 0; dy = 0; dz = 1; }
      const distance = Math.max(120, 120 * graph.height() / graph.width());
      const scale = distance / (length || 1);
      graph.cameraPosition({ x: node.x + dx * scale, y: node.y + dy * scale, z: node.z + dz * scale }, node, duration());
    } else {
      if (![...nodes.values()].every(finite)) return;
      graph.zoomToFit(duration(), 36);
    }
    needsFrame = false;
  }
  function pause() { safely(() => { active = false; graph!.pauseAnimation(); clearInterval(timer); }); }
  function visibility() {
    safely(() => {
      const size = container.getBoundingClientRect();
      const next = !document.hidden && intersecting && size.width > 0 && size.height > 0;
      if (!next) { pause(); return; }
      if (active) return;
      active = true; lastFrame = -1; stalled = 0;
      graph!.resumeAnimation();
      clearInterval(timer);
      timer = setInterval(() => safely(() => {
        if (!active || document.hidden) return;
        if (!finite(graph!.cameraPosition()) || !finite(target())) throw new Error("Invalid graph camera");
        const current = graph!.renderer().info.render.frame;
        stalled = current === lastFrame ? stalled + 1 : 0;
        lastFrame = current;
        if (stalled >= 3) throw new Error("Graph rendering stopped");
      }), 1000);
      if (needsFrame) frame();
    });
  }
  function resize() {
    safely(() => {
      const { width, height } = container.getBoundingClientRect();
      if (width > 0 && height > 0 && (width !== graph!.width() || height !== graph!.height())) {
        graph!.width(width).height(height); needsFrame = true;
      }
      visibility();
      if (needsFrame) frame();
    });
  }
  try {
    // Own the canvas so context loss and constructor failures have a bounded lifetime.
    canvas.addEventListener("webglcontextlost", fail);
    // The library exports a generic instance but a non-generic constructor.
    graph = new ForceGraph3D(container, { controlType: "orbit", rendererConfig: { canvas, antialias: true, powerPreference: "low-power" } }) as unknown as ForceGraph3DInstance<GraphNode, GraphLink>;
    const glow = document.createElement("canvas"); glow.width = glow.height = 64;
    const context = glow.getContext("2d");
    if (!context) throw new Error("Graph texture unavailable");
    const gradient = context.createRadialGradient(32, 32, 0, 32, 32, 32);
    gradient.addColorStop(0, "rgba(255,255,255,1)"); gradient.addColorStop(0.15, "rgba(255,255,255,1)");
    gradient.addColorStop(0.35, "rgba(255,255,255,0.3)"); gradient.addColorStop(1, "rgba(255,255,255,0)");
    context.fillStyle = gradient; context.fillRect(0, 0, 64, 64);
    graph.showNavInfo(false).enableNodeDrag(false).nodeLabel(node => label(node.title))
      .nodeThreeObject(node => {
        const star: Star = { group: new Group(), sprite: new Sprite(new SpriteMaterial({ map: new CanvasTexture(glow), transparent: true, depthWrite: false, toneMapped: false })) };
        star.group.add(star.sprite); stars.set(node.id, star); paintStar(node.id, star); return star.group;
      })
      .linkWidth(0.5).linkOpacity(0.6).linkDirectionalArrowLength(2).linkDirectionalArrowRelPos(0.8)
      .linkLabel(link => label(`${nodes.get(idOf(link.source))?.title ?? ""} → ${nodes.get(idOf(link.target))?.title ?? ""}`))
      .onNodeClick(node => onSelect(node.id))
      .onNodeHover(node => safely(() => { hovered = node?.id ?? null; paint(); }))
      .warmupTicks(80).cooldownTicks(40).onEngineTick(cameraChanged).onEngineStop(() => safely(() => { if (needsFrame) frame(); }));
    (graph.controls() as { addEventListener(type: string, callback: () => void): void }).addEventListener("change", cameraChanged);
    graph.renderer().setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
    canvas.setAttribute("aria-label", "3D 문서 참조 관계");
    theme(); resize();
    resizeObserver = new ResizeObserver(resize); resizeObserver.observe(container);
    intersectionObserver = new IntersectionObserver(entries => { intersecting = entries[0]?.isIntersecting ?? false; visibility(); });
    intersectionObserver.observe(container);
    themeObserver = new MutationObserver(() => safely(theme));
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style"] });
    themeObserver.observe(document.body, { attributes: true, attributeFilter: ["class", "style"] });
    document.addEventListener("visibilitychange", visibility);
    window.addEventListener("pagehide", pause); window.addEventListener("pageshow", visibility);
  } catch (error) { dispose(); throw error; }

  return {
    update(pages, edges, nextSelected, groups) {
      safely(() => {
        const ids = new Set(pages.map(page => page.id));
        slots = new Map(pages.map(page => [page.id, folderSlotOf(groups, page.path)]));
        const links = edges.filter(edge => ids.has(edge.source) && ids.has(edge.target)).map(({ source, target }) => ({ source, target }));
        const nextSignature = JSON.stringify([pages.map(({ id, title }) => [id, title]), links]);
        const changed = signature !== nextSignature;
        const selectionChanged = selected !== nextSelected;
        if (signature && selectionChanged) focusSelection = true;
        selected = nextSelected; hovered = null;
        if (changed) {
          signature = nextSignature;
          nodes = new Map(pages.map(page => [page.id, Object.assign(nodes.get(page.id) ?? {}, { id: page.id, title: page.title })]));
          for (const [id, star] of stars) if (!ids.has(id)) { releaseStar(star); stars.delete(id); }
          graph!.graphData({ nodes: [...nodes.values()], links });
        }
        paint();
        if (changed || selectionChanged) { needsFrame = true; frame(); }
      });
    },
    zoom(factor) {
      safely(() => {
        if (!Number.isFinite(factor) || factor <= 0) return;
        const camera = graph!.cameraPosition(), center = target();
        if (!finite(camera) || !finite(center)) throw new Error("Invalid graph camera");
        const distance = Math.hypot(camera.x - center.x, camera.y - center.y, camera.z - center.z) || 1;
        const scale = Math.max(20, Math.min(5000, distance * factor)) / distance;
        graph!.cameraPosition({ x: center.x + (camera.x - center.x) * scale, y: center.y + (camera.y - center.y) * scale, z: center.z + (camera.z - center.z) * scale }, center, duration());
      });
    },
    reset() { safely(() => { focusSelection = false; needsFrame = true; frame(); }); },
    dispose,
  };
}
