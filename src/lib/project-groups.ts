/** 사이드바 프로젝트 그룹의 저장 형식과 순수 조작. */

import { isProjectGroupColorId, type ProjectGroupColorId } from "./project-group-colors";

export interface ProjectGroup {
  id: string;
  name: string;
  collapsed: boolean;
  parentId?: string | null;
  color?: ProjectGroupColorId;
}

export interface ProjectGroups {
  version: 1;
  groups: ProjectGroup[];
  assignment: Record<string, string>;
}

export const PROJECT_GROUPS_STORAGE_KEY = "praxis-project-groups";
export const EMPTY_PROJECT_GROUPS: ProjectGroups = { version: 1, groups: [], assignment: {} };

const parentOf = (group: ProjectGroup): string | null => group.parentId ?? null;

const isGroup = (value: unknown): value is ProjectGroup => {
  if (typeof value !== "object" || value == null) return false;
  const { id, name, collapsed } = value as Partial<ProjectGroup>;
  return typeof id === "string" && typeof name === "string" && typeof collapsed === "boolean";
};

const parseGroup = (value: unknown): ProjectGroup | null => {
  if (!isGroup(value)) return null;
  const { id, name, collapsed, parentId, color } = value as ProjectGroup & {
    parentId?: unknown;
    color?: unknown;
  };
  const group: ProjectGroup = { id, name, collapsed };
  if (parentId === null || typeof parentId === "string") group.parentId = parentId;
  else if (parentId !== undefined) group.parentId = null;
  if (isProjectGroupColorId(color)) group.color = color;
  return group;
};

function repairGroups(groups: ProjectGroup[]): ProjectGroup[] {
  const seen = new Set<string>();
  const unique = groups.filter((group) => {
    if (seen.has(group.id)) return false;
    seen.add(group.id);
    return true;
  }).map((group) => ({ ...group }));
  const byId = new Map(unique.map((group) => [group.id, group]));
  for (const group of unique) {
    if (group.parentId !== undefined && group.parentId !== null && (typeof group.parentId !== "string" || group.parentId === group.id || !byId.has(group.parentId))) {
      group.parentId = null;
    }
  }
  for (const group of unique) {
    const path = new Set<string>();
    let current = group;
    while (parentOf(current) !== null) {
      if (path.has(current.id)) {
        current.parentId = null;
        break;
      }
      path.add(current.id);
      const parent = byId.get(parentOf(current)!);
      if (parent == null) {
        current.parentId = null;
        break;
      }
      current = parent;
    }
  }
  return unique;
}

export function parseProjectGroups(raw: string | null): ProjectGroups {
  if (raw == null) return EMPTY_PROJECT_GROUPS;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed == null) return EMPTY_PROJECT_GROUPS;
    const { version, groups, assignment } = parsed as Partial<ProjectGroups>;
    const parsedGroups = Array.isArray(groups) ? groups.map(parseGroup) : null;
    if (version !== 1 || parsedGroups == null || !parsedGroups.every((group): group is ProjectGroup => group !== null)) {
      return EMPTY_PROJECT_GROUPS;
    }
    if (typeof assignment !== "object" || assignment == null) return EMPTY_PROJECT_GROUPS;
    if (!Object.values(assignment).every((id) => typeof id === "string")) return EMPTY_PROJECT_GROUPS;
    return { version: 1, groups: repairGroups(parsedGroups), assignment };
  } catch {
    return EMPTY_PROJECT_GROUPS;
  }
}

export const serializeProjectGroups = (groups: ProjectGroups): string => JSON.stringify(groups);

export function loadProjectGroups(): ProjectGroups {
  try {
    return parseProjectGroups(localStorage.getItem(PROJECT_GROUPS_STORAGE_KEY));
  } catch {
    return EMPTY_PROJECT_GROUPS;
  }
}

export function saveProjectGroups(groups: ProjectGroups): void {
  try {
    localStorage.setItem(PROJECT_GROUPS_STORAGE_KEY, serializeProjectGroups(groups));
  } catch (error) {
    console.warn("프로젝트 그룹을 저장하지 못했습니다", error);
  }
}

export const createGroup = (groups: ProjectGroups, name: string, id: string): ProjectGroups => ({
  ...groups,
  groups: [...groups.groups, { id, name, collapsed: false, parentId: null }],
});

export const renameGroup = (groups: ProjectGroups, id: string, name: string): ProjectGroups => ({
  ...groups,
  groups: groups.groups.map((group) => (group.id === id ? { ...group, name } : group)),
});

export const toggleGroup = (groups: ProjectGroups, id: string): ProjectGroups => ({
  ...groups,
  groups: groups.groups.map((group) => group.id === id ? { ...group, collapsed: !group.collapsed } : group),
});

export const setGroupColor = (
  groups: ProjectGroups,
  id: string,
  color: ProjectGroupColorId | undefined,
): ProjectGroups => ({
  ...groups,
  groups: groups.groups.map((group) => {
    if (group.id !== id) return group;
    if (color !== undefined) return { ...group, color };
    const { color: _color, ...withoutColor } = group;
    return withoutColor;
  }),
});

/** 해당 그룹만 없애고, 직접 프로젝트와 자식을 바로 위 부모로 승격한다. */
export function dissolveGroup(groups: ProjectGroups, id: string): ProjectGroups {
  const removed = groups.groups.find((group) => group.id === id);
  if (removed == null) return groups;
  const parentId = parentOf(removed);
  const assignment: Record<string, string> = {};
  for (const [repo, groupId] of Object.entries(groups.assignment)) {
    if (groupId === id && parentId === null) continue;
    assignment[repo] = groupId === id ? parentId! : groupId;
  }
  return {
    ...groups,
    groups: groups.groups.filter((group) => group.id !== id)
      .map((group) => (parentOf(group) === id ? { ...group, parentId } : group)),
    assignment,
  };
}

export const assignProject = (groups: ProjectGroups, repo: string, groupId: string): ProjectGroups => ({
  ...groups,
  assignment: { ...groups.assignment, [repo]: groupId },
});

export function unassignProject(groups: ProjectGroups, repo: string): ProjectGroups {
  if (!(repo in groups.assignment)) return groups;
  const assignment = { ...groups.assignment };
  delete assignment[repo];
  return { ...groups, assignment };
}

export function canMoveGroup(groups: ProjectGroups, groupId: string, targetParentId: string | null): boolean {
  const byId = new Map(groups.groups.map((group) => [group.id, group]));
  if (!byId.has(groupId) || (targetParentId !== null && !byId.has(targetParentId))) return false;
  let current = targetParentId === null ? null : byId.get(targetParentId)!;
  const path = new Set<string>();
  while (current != null) {
    if (current.id === groupId || path.has(current.id)) return false;
    path.add(current.id);
    current = parentOf(current) === null ? null : byId.get(parentOf(current)!) ?? null;
  }
  return true;
}

export function moveGroup(
  groups: ProjectGroups,
  groupId: string,
  targetParentId: string | null,
  siblingIndex?: number,
): ProjectGroups {
  if (!canMoveGroup(groups, groupId, targetParentId)) return groups;
  const moved = groups.groups.find((group) => group.id === groupId)!;
  const without = groups.groups.filter((group) => group.id !== groupId);
  const siblings = without.filter((group) => parentOf(group) === targetParentId);
  const index = siblingIndex ?? siblings.length;
  if (!Number.isInteger(index) || index < 0 || index > siblings.length) return groups;
  const before = siblings[index];
  const after = siblings[siblings.length - 1];
  const position = before == null ? (after == null ? without.length : without.indexOf(after) + 1) : without.indexOf(before);
  const next = [...without];
  next.splice(position, 0, { ...moved, parentId: targetParentId });
  return next.every((group, position) => group === groups.groups[position]) ? groups : { ...groups, groups: next };
}

export { orderSections, visibleProjectRepos } from "./project-group-tree";
export type { ProjectSection } from "./project-group-tree";
