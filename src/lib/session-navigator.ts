import type { Task } from "./ipc";
import { orderSections, type ProjectGroups } from "./project-groups";
import { taskKey } from "./transport";

export type SessionNavigatorKind = "group" | "project" | "session" | "empty";

export interface SessionNavigatorNode {
  id: string;
  kind: SessionNavigatorKind;
  label: string;
  searchText: string;
  task?: Task;
  children: SessionNavigatorNode[];
}

export interface SessionNavigatorRow extends SessionNavigatorNode {
  depth: number;
}

const repoLabel = (repo: string): string => repo.split("/").filter(Boolean).pop() ?? repo;

const sessionLabel = (task: Task): string => task.instruction || task.branch;

const emptyNode = (id: string, label: string): SessionNavigatorNode => ({
  id,
  kind: "empty",
  label,
  searchText: label,
  children: [],
});

const projectNode = (repo: string, tasks: Task[]): SessionNavigatorNode => {
  const children = tasks.map((task) => ({
    id: `session:${taskKey(task)}`,
    kind: "session" as const,
    label: sessionLabel(task),
    searchText: `${sessionLabel(task)} ${task.branch} ${task.host}`,
    task,
    children: [],
  }));
  return {
    id: `project:${repo}`,
    kind: "project",
    label: repoLabel(repo),
    searchText: `${repoLabel(repo)} ${repo}`,
    children: children.length ? children : [emptyNode(`empty:${repo}`, "세션 없음")],
  };
};

/** 사이드바와 같은 그룹·프로젝트 순서를 유지한 읽기 전용 트리. */
export function buildSessionNavigatorTree(
  projects: string[],
  tasks: Task[],
  groups: ProjectGroups,
): SessionNavigatorNode[] {
  return orderSections(projects, tasks, groups).map((section) => {
    const label = section.group?.name ?? "미소속 프로젝트";
    const children = section.repos.map((repo) =>
      projectNode(repo, tasks.filter((task) => task.repo === repo)),
    );
    return {
      id: section.group ? `group:${section.group.id}` : "group:unassigned",
      kind: "group",
      label,
      searchText: label,
      children: children.length
        ? children
        : [emptyNode(`empty:${section.group?.id ?? "unassigned"}`, "프로젝트 없음")],
    };
  });
}

const matches = (node: SessionNavigatorNode, query: string): boolean =>
  node.searchText.toLocaleLowerCase().includes(query);

const filterNode = (node: SessionNavigatorNode, query: string): SessionNavigatorNode | null => {
  const children = node.children
    .map((child) => filterNode(child, query))
    .filter((child): child is SessionNavigatorNode => child !== null);
  if (!matches(node, query) && children.length === 0) return null;
  return { ...node, children: matches(node, query) ? node.children : children };
};

/** 검색은 일치한 항목과 조상을 남기며, 조상 자체가 일치하면 그 하위 경로를 모두 남긴다. */
export function filterSessionNavigatorTree(
  nodes: SessionNavigatorNode[],
  query: string,
): SessionNavigatorNode[] {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return nodes;
  return nodes
    .map((node) => filterNode(node, normalized))
    .filter((node): node is SessionNavigatorNode => node !== null);
}

export function sessionNavigatorRows(
  nodes: SessionNavigatorNode[],
  collapsed: ReadonlySet<string>,
  searching: boolean,
): SessionNavigatorRow[] {
  const rows: SessionNavigatorRow[] = [];
  const visit = (node: SessionNavigatorNode, depth: number): void => {
    rows.push({ ...node, depth });
    if (node.kind !== "empty" && (searching || !collapsed.has(node.id))) {
      node.children.forEach((child) => visit(child, depth + 1));
    }
  };
  nodes.forEach((node) => visit(node, 0));
  return rows;
}
