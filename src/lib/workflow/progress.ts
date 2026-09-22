import type { Task } from "../ipc";
import { isInFlight, needsUser } from "../task-status";

/** View annotations only. Task state continues to come from the existing task list. */
export interface ProgressView {
  version: 1;
  phases: string[];
  phaseByTask: Record<string, string>;
  dependencies: Array<{ from: number; to: number }>;
}
export interface ProgressEdge { from: number; to: number; kind: "dependency" | "continuation"; }
export const UNASSIGNED = "미분류";
export const NODE_WIDTH = 200;
export const NODE_HEIGHT = 96;
export const COLUMN_GAP = 48;
const ROW_GAP = 16;
const GRAPH_PADDING = 16;

export function emptyProgressView(): ProgressView {
  return { version: 1, phases: ["계획", "구현", "검증"], phaseByTask: {}, dependencies: [] };
}
export function progressStorageKey(host: string, repo: string): string {
  return `praxis-progress-v1:${JSON.stringify([host, repo])}`;
}
const isId = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) > 0;
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);

export function parseProgressView(text: string): ProgressView {
  if (text.length > 512 * 1024) throw new Error("그래프 설정이 너무 큽니다.");
  const value: unknown = JSON.parse(text);
  if (!record(value) || value.version !== 1 || !Array.isArray(value.phases) || value.phases.length > 32
    || !value.phases.every((phase) => typeof phase === "string" && phase.trim() === phase && phase.length > 0 && phase.length <= 80 && phase !== UNASSIGNED)
    || new Set(value.phases).size !== value.phases.length || !record(value.phaseByTask)
    || Object.keys(value.phaseByTask).length > 10_000 || !Array.isArray(value.dependencies) || value.dependencies.length > 2_000) {
    throw new Error("저장된 그래프 설정을 읽을 수 없습니다.");
  }
  for (const [id, phase] of Object.entries(value.phaseByTask)) {
    if (!/^[1-9]\d*$/.test(id) || !isId(Number(id)) || !value.phases.includes(phase)) throw new Error("Phase 지정이 올바르지 않습니다.");
  }
  if (!value.dependencies.every((edge) => record(edge) && isId(edge.from) && isId(edge.to) && edge.from !== edge.to)) {
    throw new Error("작업 연결이 올바르지 않습니다.");
  }
  return {
    version: 1,
    phases: value.phases as string[],
    phaseByTask: value.phaseByTask as Record<string, string>,
    dependencies: value.dependencies.map((edge) => ({ from: edge.from as number, to: edge.to as number })),
  };
}

export function canConnect(edges: readonly ProgressEdge[], from: number, to: number): boolean {
  if (from === to || edges.some((edge) => edge.from === from && edge.to === to)) return false;
  const next = new Map<number, number[]>();
  for (const edge of edges) next.set(edge.from, [...(next.get(edge.from) ?? []), edge.to]);
  const stack = [to];
  const seen = new Set<number>();
  while (stack.length) {
    const current = stack.pop()!;
    if (current === from) return false;
    if (seen.has(current)) continue;
    seen.add(current);
    stack.push(...(next.get(current) ?? []));
  }
  return true;
}

/** Scope before resolving IDs: the same task ID exists on different hosts. */
export function progressGraph(allTasks: readonly Task[], host: string, repo: string, view: ProgressView) {
  const tasks = allTasks.filter((task) => task.host === host && task.repo === repo)
    .sort((left, right) => left.created_at - right.created_at || left.id - right.id);
  const ids = new Set(tasks.map((task) => task.id));
  const edges: ProgressEdge[] = [];
  let omitted = 0;
  const candidates: ProgressEdge[] = [
    ...tasks.filter((task) => task.resumed_from != null).map((task) => ({ from: task.resumed_from!, to: task.id, kind: "continuation" as const })),
    ...view.dependencies.map((edge) => ({ ...edge, kind: "dependency" as const })),
  ];
  for (const edge of candidates) {
    if (!ids.has(edge.from) || !ids.has(edge.to)) continue;
    if (canConnect(edges, edge.from, edge.to)) edges.push(edge);
    else omitted++;
  }
  const incoming = new Map(tasks.map((task) => [task.id, 0]));
  const outgoing = new Map<number, number[]>();
  const ranks = new Map<number, number>();
  for (const edge of edges) {
    incoming.set(edge.to, incoming.get(edge.to)! + 1);
    outgoing.set(edge.from, [...(outgoing.get(edge.from) ?? []), edge.to]);
  }
  const ready = tasks.filter((task) => incoming.get(task.id) === 0).map((task) => task.id);
  for (let index = 0; index < ready.length; index++) {
    const id = ready[index];
    for (const to of outgoing.get(id) ?? []) {
      ranks.set(to, Math.max(ranks.get(to) ?? 0, (ranks.get(id) ?? 0) + 1));
      incoming.set(to, incoming.get(to)! - 1);
      if (incoming.get(to) === 0) ready.push(to);
    }
  }
  const rows = new Map<number, number>();
  const nodes = tasks.map((task) => {
    const column = ranks.get(task.id) ?? 0;
    const row = rows.get(column) ?? 0;
    rows.set(column, row + 1);
    return { task, phase: view.phaseByTask[String(task.id)] ?? UNASSIGNED, x: GRAPH_PADDING + column * (NODE_WIDTH + COLUMN_GAP), y: GRAPH_PADDING + row * (NODE_HEIGHT + ROW_GAP) };
  });
  return {
    nodes, edges, omitted,
    width: nodes.reduce((width, node) => Math.max(width, node.x + NODE_WIDTH + GRAPH_PADDING), 280),
    height: nodes.reduce((height, node) => Math.max(height, node.y + NODE_HEIGHT + GRAPH_PADDING), 160),
    counts: {
      total: tasks.length,
      done: tasks.filter((task) => !task.stale && task.state === "Done").length,
      running: tasks.filter((task) => !task.stale && isInFlight(task)).length,
      awaiting: tasks.filter((task) => !task.stale && needsUser(task)).length,
      failed: tasks.filter((task) => !task.stale && task.state === "Failed").length,
      stale: tasks.filter((task) => task.stale).length,
    },
  };
}
