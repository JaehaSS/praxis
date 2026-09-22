import { describe, expect, it } from "vitest";

import { layoutCodeGraph } from "./code-graph-layout";

const nodes = [1, 2, 3].map((id) => ({ id, name: `node${id}`, relPath: `src/${id}.ts`, line: 0, character: 0 }));
const edges = [
  { sourceId: 1, targetId: 2, relation: "references" as const },
  { sourceId: 2, targetId: 3, relation: "references" as const },
];

describe("layoutCodeGraph", () => {
  it("places incoming references in deterministic depth columns", () => {
    expect(layoutCodeGraph(nodes, edges, 3, "incoming")).toEqual([
      { id: 1, x: 528, y: 48, depth: 2 },
      { id: 2, x: 288, y: 48, depth: 1 },
      { id: 3, x: 48, y: 48, depth: 0 },
    ]);
  });

  it("does not invent an edge between nodes at the same depth", () => {
    const positions = layoutCodeGraph(
      [...nodes, { id: 4, name: "node4", relPath: "src/4.ts", line: 0, character: 0 }],
      [...edges, { sourceId: 4, targetId: 3, relation: "references" }],
      3,
      "incoming",
    );

    expect(positions.filter((position) => position.depth === 1).map((position) => position.id)).toEqual([2, 4]);
  });
});
