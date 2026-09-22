// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CodeGraphReport, CodeGraphStatus } from "../../lib/ipc";
import { useCodeGraphPanel, type EditorCodeGraphActions } from "./useCodeGraphPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

const status = (runId: number): CodeGraphStatus => ({
  activeState: "ready",
  activeRunId: runId,
  indexedAt: runId,
  files: 1,
  symbols: 1,
  edges: 0,
  buildState: "idle",
  buildRunId: null,
  detail: null,
  incomplete: null,
});

function actions(scope: number, load: () => Promise<CodeGraphStatus>): EditorCodeGraphActions {
  return {
    scope,
    status: load,
    index: async () => ({
      runId: scope,
      state: "ready",
      filesSeen: 1,
      filesIndexed: 1,
      filesUnchanged: 0,
      filesSkipped: 0,
      symbols: 1,
      edges: 0,
    }),
    cancel: async () => undefined,
    impactAt: async () => ({
      runId: scope,
      indexedAt: scope,
      freshness: "ready",
      items: [],
      truncated: false,
      edgesUnavailable: null,
    }),
    neighborhoodAt: async () => ({
      runId: scope,
      indexedAt: scope,
      freshness: "ready",
      rootId: scope,
      nodes: [],
      edges: [],
      truncated: false,
      incomplete: null,
      edgesUnavailable: null,
      encounteredIncomplete: [],
    }),
    openItem: async () => undefined,
    openNode: async () => undefined,
  };
}

function Harness({ path, graph }: { path: string; graph: EditorCodeGraphActions }) {
  const panel = useCodeGraphPanel({
    actions: graph,
    path,
    dirty: false,
    getPosition: () => null,
  });
  return (
    <button data-status={panel.status?.activeRunId ?? "pending"} data-build={panel.status?.buildState ?? "pending"} data-busy={panel.busy} onClick={() => void panel.index()}>
      index
    </button>
  );
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  vi.useRealTimers();
  act(() => root.unmount());
  container.remove();
});

describe("useCodeGraphPanel", () => {
  it("clears an in-flight index on scope switch without reviving it from the old scope", async () => {
    const indexing = deferred<CodeGraphReport>();
    const first = { ...actions(1, async () => status(1)), index: () => indexing.promise };
    await act(async () => root.render(<Harness path="src/a.ts" graph={first} />));
    await act(async () => container.querySelector("button")?.click());
    expect(container.querySelector("button")?.dataset.busy).toBe("true");
    await act(async () => root.render(<Harness path="src/a.ts" graph={actions(2, async () => status(2))} />));
    expect(container.querySelector("button")?.dataset.busy).toBe("false");
    await act(async () => indexing.resolve({ runId: 1, state: "ready", filesSeen: 0, filesIndexed: 0, filesUnchanged: 0, filesSkipped: 0, symbols: 0, edges: 0 }));
    expect(container.querySelector("button")?.dataset.status).toBe("2");
  });

  it("rejects a late graph after source edit and blocks direction changes until a fresh source query", async () => {
    const graph = actions(1, async () => status(1));
    const result = await graph.neighborhoodAt({ path: "src/a.ts", line: 3, column: 1, direction: "incoming", depth: 1 });
    const pending = deferred<typeof result>();
    graph.neighborhoodAt = vi.fn().mockReturnValueOnce(pending.promise).mockResolvedValue(result);
    let panel!: ReturnType<typeof useCodeGraphPanel>;
    function QueryHarness() {
      panel = useCodeGraphPanel({ actions: graph, path: "src/a.ts", dirty: false, getPosition: () => ({ line: 3, column: 1 }) });
      return null;
    }
    await act(async () => root.render(<QueryHarness />));
    act(() => { void panel.inspectNeighborhood("incoming", 1); });
    act(() => panel.invalidate());
    await act(async () => pending.resolve(result));
    expect(panel.neighborhood).toBeNull();
    await act(async () => panel.inspectNeighborhood("outgoing", 2));
    expect(graph.neighborhoodAt).toHaveBeenCalledTimes(1);
    expect(panel.error).toContain("저장 후");
    await act(async () => panel.inspectNeighborhood("incoming", 1, { path: "src/a.ts", line: 3, column: 1, dirty: false }));
    expect(graph.neighborhoodAt).toHaveBeenCalledTimes(2);
    expect(panel.neighborhood).toEqual(result);
  });

  it("should block an impact query when its source has unsaved changes", async () => {
    const graph = actions(1, async () => status(1));
    graph.impactAt = vi.fn(graph.impactAt);
    let panel!: ReturnType<typeof useCodeGraphPanel>;
    function QueryHarness() {
      panel = useCodeGraphPanel({ actions: graph, path: "src/a.ts", dirty: true, getPosition: () => ({ line: 3, column: 1 }) });
      return null;
    }

    await act(async () => root.render(<QueryHarness />));
    await act(async () => panel.inspect());

    expect(graph.impactAt).not.toHaveBeenCalled();
    expect(panel.error).toContain("저장 후");
  });

  it("should block an impact query when its source was invalidated", async () => {
    const graph = actions(1, async () => status(1));
    graph.impactAt = vi.fn(graph.impactAt);
    let panel!: ReturnType<typeof useCodeGraphPanel>;
    function QueryHarness() {
      panel = useCodeGraphPanel({ actions: graph, path: "src/a.ts", dirty: false, getPosition: () => ({ line: 3, column: 1 }) });
      return null;
    }

    await act(async () => root.render(<QueryHarness />));
    act(() => panel.invalidate());
    await act(async () => panel.inspect());

    expect(graph.impactAt).not.toHaveBeenCalled();
    expect(panel.error).toContain("저장 후");
  });

  it("discards a late status response after its path and scope change", async () => {
    const first = deferred<CodeGraphStatus>();
    const second = deferred<CodeGraphStatus>();

    await act(async () => {
      root.render(<Harness path="src/a.ts" graph={actions(1, () => first.promise)} />);
    });
    await act(async () => {
      root.render(<Harness path="src/b.ts" graph={actions(2, () => second.promise)} />);
    });
    await act(async () => first.resolve(status(1)));

    expect(container.querySelector("button")?.dataset.status).toBe("pending");

    await act(async () => second.resolve(status(2)));

    expect(container.querySelector("button")?.dataset.status).toBe("2");
  });

  it("clears busy after a long index even while polling reloads status", async () => {
    vi.useFakeTimers();
    const indexing = deferred<CodeGraphReport>();
    const graph = { ...actions(1, async () => status(1)), index: () => indexing.promise };

    await act(async () => {
      root.render(<Harness path="src/a.ts" graph={graph} />);
    });
    await act(async () => {
      container.querySelector("button")?.click();
    });
    await act(async () => vi.advanceTimersByTimeAsync(1500));

    await act(async () => indexing.resolve({
      runId: 1,
      state: "ready",
      filesSeen: 1,
      filesIndexed: 1,
      filesUnchanged: 0,
      filesSkipped: 0,
      symbols: 1,
      edges: 0,
    }));

    expect(container.querySelector("button")?.dataset.busy).toBe("false");
    vi.useRealTimers();
  });

  it("should poll an externally started index until the backend reports a terminal state", async () => {
    vi.useFakeTimers();
    const load = vi
      .fn<() => Promise<CodeGraphStatus>>()
      .mockResolvedValueOnce({ ...status(1), buildState: "indexing_symbols", buildRunId: 2 })
      .mockResolvedValue(status(1));

    await act(async () => {
      root.render(<Harness path="src/a.ts" graph={actions(1, load)} />);
    });
    await act(async () => vi.advanceTimersByTimeAsync(750));
    await act(async () => vi.advanceTimersByTimeAsync(750));

    expect(load).toHaveBeenCalledTimes(2);
    expect(container.querySelector("button")?.dataset.build).toBe("idle");
  });

  it("should stop externally started index polling when the popout unmounts", async () => {
    vi.useFakeTimers();
    const load = vi.fn<() => Promise<CodeGraphStatus>>().mockResolvedValue({
      ...status(1),
      buildState: "indexing_symbols",
      buildRunId: 2,
    });

    await act(async () => {
      root.render(<Harness path="src/a.ts" graph={actions(1, load)} />);
    });
    act(() => root.unmount());
    await act(async () => vi.advanceTimersByTimeAsync(750));

    expect(load).toHaveBeenCalledTimes(1);
  });
});
