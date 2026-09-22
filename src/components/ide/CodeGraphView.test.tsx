// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CodeGraphNeighborhood } from "../../lib/ipc";
import { CodeGraphView } from "./CodeGraphView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const graph: CodeGraphNeighborhood = {
  runId: 1,
  indexedAt: 1,
  freshness: "ready",
  rootId: 2,
  nodes: [
    { id: 1, name: "caller", relPath: "src/a.ts", line: 2, character: 0 },
    { id: 2, name: "target", relPath: "src/b.ts", line: 4, character: 0 },
  ],
  edges: [{ sourceId: 1, targetId: 2, relation: "references" }],
  truncated: false,
  incomplete: null,
  edgesUnavailable: null,
  encounteredIncomplete: [],
};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
});

describe("CodeGraphView", () => {
  it("defaults a narrow editor area to the list and supports an explicit graph switch", () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ width: 480 } as DOMRect);
    const onClose = vi.fn();
    act(() => root.render(<CodeGraphView graph={graph} error={null} direction="incoming" depth={1} onDirection={vi.fn()} onDepth={vi.fn()} onOpen={vi.fn()} onClose={onClose} />));
    expect(container.querySelector("svg")).toBeNull();
    expect(container.querySelector<HTMLElement>('[aria-label="참조 그래프 목록"]')?.hidden).toBe(false);
    act(() => [...container.querySelectorAll("button")].find((button) => button.textContent === "그래프")?.click());
    expect(container.querySelectorAll("line")).toHaveLength(1);
    act(() => container.querySelector("aside")?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("draws only stored reference edges and opens a clicked node", () => {
    const onOpen = vi.fn();
    act(() => {
      root.render(<CodeGraphView graph={graph} error={null} direction="incoming" depth={1} onDirection={vi.fn()} onDepth={vi.fn()} onOpen={onOpen} onClose={vi.fn()} />);
    });

    expect(container.querySelectorAll("line")).toHaveLength(1);
    expect(container.textContent).toContain("caller");
    expect(container.textContent).toContain("target");
    expect(container.textContent).toContain("기준: target · src/b.ts:5");
    act(() => {
      container.querySelectorAll("g")[0]?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(onOpen).toHaveBeenCalledWith(graph.nodes[0]);
  });

  it("keeps partial-index facts and keyboard list visible", () => {
    act(() => {
      root.render(<CodeGraphView graph={{ ...graph, truncated: true, incomplete: { filesSkipped: 1, filesWithoutEdges: 0, languagesWithoutEdges: [], detail: "인덱스가 완료되지 않았습니다" }, encounteredIncomplete: [{ relPath: "src/broken.ts", reason: "일부 참조를 읽지 못했습니다" }] }} error={null} direction="incoming" depth={1} onDirection={vi.fn()} onDepth={vi.fn()} onOpen={vi.fn()} onClose={vi.fn()} />);
    });

    expect(container.textContent).toContain("결과가 일부만 표시되었습니다.");
    expect(container.textContent).toContain("src/broken.ts: 일부 참조를 읽지 못했습니다");
    expect(container.querySelector('[aria-label="참조 그래프 목록"]')).not.toBeNull();
  });

  it("shows an empty state instead of the canvas when edges are unavailable and there are none", () => {
    const unavailableGraph: CodeGraphNeighborhood = { ...graph, edges: [], edgesUnavailable: "pyright가 준비되지 않아 참조를 분석하지 못했습니다" };
    act(() => {
      root.render(<CodeGraphView graph={unavailableGraph} error={null} direction="incoming" depth={1} onDirection={vi.fn()} onDepth={vi.fn()} onOpen={vi.fn()} onClose={vi.fn()} />);
    });

    expect(container.querySelector("svg")).toBeNull();
    const status = container.querySelector('[role="status"]');
    expect(status?.textContent).toContain("이 파일은 참조를 분석하지 못했습니다");
    expect(status?.textContent).toContain(unavailableGraph.edgesUnavailable);
    expect(container.textContent?.split(unavailableGraph.edgesUnavailable as string).length).toBe(2);
    expect(container.textContent).not.toContain("저장된 참조가 없습니다");
    const list = container.querySelector('[aria-label="참조 그래프 목록"]');
    expect(list).not.toBeNull();
    expect(list?.textContent).toContain("target");
  });

  it("keeps the plain zero-reference canvas when edgesUnavailable is null", () => {
    const emptyGraph: CodeGraphNeighborhood = { ...graph, edges: [], edgesUnavailable: null };
    act(() => {
      root.render(<CodeGraphView graph={emptyGraph} error={null} direction="incoming" depth={1} onDirection={vi.fn()} onDepth={vi.fn()} onOpen={vi.fn()} onClose={vi.fn()} />);
    });

    expect(container.querySelector("svg")).not.toBeNull();
    expect(container.textContent).toContain("저장된 참조가 없습니다");
    expect(container.textContent).not.toContain("분석하지 못했습니다");
  });
});
