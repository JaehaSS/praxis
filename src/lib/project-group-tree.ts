import type { Task } from "./ipc";
import type { ProjectGroup, ProjectGroups } from "./project-groups";

export interface ProjectSection {
  group: ProjectGroup | null;
  repos: string[];
  children: ProjectSection[];
}

const parentOf = (group: ProjectGroup): string | null => group.parentId ?? null;

/** 마지막 작업이 최근인 프로젝트가 위로. 작업이 없는 프로젝트는 맨 아래. */
const sortByLastUse = (projects: string[], tasks: Task[]): string[] => {
  const lastUsed = new Map<string, number>();
  for (const task of tasks) {
    if (task.created_at > (lastUsed.get(task.repo) ?? 0)) lastUsed.set(task.repo, task.created_at);
  }
  return [...projects].sort(
    (left, right) => (lastUsed.get(right) ?? -1) - (lastUsed.get(left) ?? -1),
  );
};

/** `[자식 그룹들 → 직접 프로젝트들]`을 재귀로 만들고 마지막에 미소속을 붙인다. */
export function orderSections(
  projects: string[],
  tasks: Task[],
  groups: ProjectGroups,
): ProjectSection[] {
  const ordered = sortByLastUse(projects, tasks);
  const known = new Set(groups.groups.map((group) => group.id));
  const children = new Map<string | null, ProjectGroup[]>();
  const repos = new Map<string, string[]>();
  for (const group of groups.groups) {
    const parentId = parentOf(group);
    const siblings = children.get(parentId);
    if (siblings == null) children.set(parentId, [group]);
    else siblings.push(group);
  }
  const unassigned: string[] = [];
  for (const repo of ordered) {
    const groupId = groups.assignment[repo];
    if (!known.has(groupId)) {
      unassigned.push(repo);
      continue;
    }
    const assigned = repos.get(groupId);
    if (assigned == null) repos.set(groupId, [repo]);
    else assigned.push(repo);
  }
  const build = (parentId: string | null): ProjectSection[] =>
    (children.get(parentId) ?? []).map((group) => ({
      group,
      repos: repos.get(group.id) ?? [],
      children: build(group.id),
    }));
  return [...build(null), { group: null, repos: unassigned, children: [] }];
}

/** 실제 DOM 순회와 같은 순서로, 접힌 그룹의 모든 후손을 건너뛴다. */
export function visibleProjectRepos(sections: ProjectSection[]): string[] {
  return sections.flatMap((section) => {
    if (section.group?.collapsed) return [];
    return [...visibleProjectRepos(section.children), ...section.repos];
  });
}

export function groupPath(groups: ProjectGroup[], groupId: string): string {
  const byId = new Map(groups.map((group) => [group.id, group]));
  const names: string[] = [];
  let current = byId.get(groupId);
  while (current != null) {
    names.unshift(current.name);
    current = parentOf(current) === null ? undefined : byId.get(parentOf(current)!);
  }
  return names.join(" / ");
}
