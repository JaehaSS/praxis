import type { CodeGraphDirection, CodeGraphNeighborhoodEdge, CodeGraphNeighborhoodNode } from "./ipc";

export interface GraphPosition {
  id: number;
  x: number;
  y: number;
  depth: number;
}

export function layoutCodeGraph(
  nodes: CodeGraphNeighborhoodNode[],
  edges: CodeGraphNeighborhoodEdge[],
  rootId: number,
  direction: CodeGraphDirection,
): GraphPosition[] {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const depths = new Map<number, number>([[rootId, 0]]);
  const queue = [rootId];
  while (queue.length > 0) {
    const id = queue.shift();
    if (id == null) break;
    const depth = depths.get(id) ?? 0;
    const next = edges
      .flatMap((edge) => (direction === "incoming" ? (edge.targetId === id ? [edge.sourceId] : []) : edge.sourceId === id ? [edge.targetId] : []))
      .filter((nextId) => byId.has(nextId) && !depths.has(nextId))
      .sort((left, right) => left - right);
    next.forEach((nextId) => {
      depths.set(nextId, depth + 1);
      queue.push(nextId);
    });
  }
  const columns = new Map<number, number[]>();
  nodes.forEach((node) => {
    const depth = depths.get(node.id) ?? 0;
    columns.set(depth, [...(columns.get(depth) ?? []), node.id]);
  });
  return Array.from(columns)
    .flatMap(([depth, ids]) => ids.sort((left, right) => left - right).map((id, index) => ({ id, x: 48 + depth * 240, y: 48 + index * 72, depth })))
    .sort((left, right) => left.id - right.id);
}
