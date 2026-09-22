/** 사이드바 프로젝트·그룹 드래그의 React 연결. */

import { useRef, useState, type DragEvent } from "react";
import {
  assignProject,
  canMoveGroup,
  moveGroup,
  unassignProject,
  type ProjectGroups,
} from "../../lib/project-groups";
import { insertIndexFromPoint } from "./editor-drag";
import {
  acceptsProjectDrop,
  draggedProjectKind,
  PROJECT_DRAG_MIME,
  PROJECT_GROUP_DRAG_MIME,
} from "./project-drag";

type DropZone = { id: string | null } | null;
type Caret = { parentId: string | null; index: number } | null;

export type ProjectGroupDrag = ReturnType<typeof useProjectGroupDrag>;

export function useProjectGroupDrag(
  groups: ProjectGroups,
  onChange: (next: ProjectGroups) => void,
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [dragged, setDragged] = useState<string | null>(null);
  const [draggedGroup, setDraggedGroup] = useState<string | null>(null);
  const [zone, setZone] = useState<DropZone>(null);
  const [caret, setCaret] = useState<Caret>(null);

  const groupOf = (repo: string): string | null => {
    const id = groups.assignment[repo];
    return id != null && groups.groups.some((group) => group.id === id) ? id : null;
  };
  const clear = (): void => {
    setDragged(null);
    setDraggedGroup(null);
    setZone(null);
    setCaret(null);
  };
  const stillInside = (event: DragEvent<HTMLElement>): boolean =>
    event.currentTarget.contains(event.relatedTarget as Node | null);

  const over = (event: DragEvent<HTMLElement>, target: string | null): void => {
    const kind = draggedProjectKind(event.dataTransfer.types);
    const accepted = kind === "group"
      ? draggedGroup !== null && canMoveGroup(groups, draggedGroup, target)
      : acceptsProjectDrop(kind, dragged === null ? null : groupOf(dragged), target);
    if (!accepted) {
      if (kind !== null) {
        event.stopPropagation();
        event.dataTransfer.dropEffect = "none";
      }
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    setCaret(null);
    setZone({ id: target });
  };

  const drop = (event: DragEvent<HTMLElement>, target: string | null): void => {
    const groupId = event.dataTransfer.getData(PROJECT_GROUP_DRAG_MIME);
    const repo = event.dataTransfer.getData(PROJECT_DRAG_MIME);
    clear();
    if (groupId !== "") {
      event.preventDefault();
      event.stopPropagation();
      if (canMoveGroup(groups, groupId, target)) onChange(moveGroup(groups, groupId, target));
      return;
    }
    if (repo === "") return;
    event.preventDefault();
    event.stopPropagation();
    if (groupOf(repo) === target) return;
    onChange(target === null ? unassignProject(groups, repo) : assignProject(groups, repo, target));
  };

  const groupList = (event: DragEvent<HTMLElement>): HTMLElement | null =>
    event.currentTarget.closest<HTMLElement>("[data-group-list]");

  const overGroups = (event: DragEvent<HTMLElement>, parentId: string | null): void => {
    if (draggedProjectKind(event.dataTransfer.types) !== "group" || draggedGroup === null) return;
    if (!canMoveGroup(groups, draggedGroup, parentId)) {
      event.stopPropagation();
      event.dataTransfer.dropEffect = "none";
      return;
    }
    const list = groupList(event);
    const boxes = Array.from(list?.querySelectorAll<HTMLElement>("[data-group-box]") ?? [])
      .filter((box) => box.closest("[data-group-list]") === list && box.dataset.groupBox !== draggedGroup);
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    setZone(null);
    setCaret({
      parentId,
      index: insertIndexFromPoint(boxes.map((box) => box.getBoundingClientRect()), event.clientY, "y"),
    });
  };

  const dropGroups = (event: DragEvent<HTMLElement>, parentId: string | null): void => {
    const id = event.dataTransfer.getData(PROJECT_GROUP_DRAG_MIME);
    const index = caret?.parentId === parentId ? caret.index : null;
    clear();
    if (id === "" || index === null) return;
    event.preventDefault();
    event.stopPropagation();
    onChange(moveGroup(groups, id, parentId, index));
  };

  const headerEdge = (event: DragEvent<HTMLElement>): "before" | "after" | null => {
    const box = event.currentTarget.getBoundingClientRect();
    const edge = Math.min(12, box.height / 3);
    if (event.clientY - box.top < edge) return "before";
    if (box.bottom - event.clientY < edge) return "after";
    return null;
  };

  const overGroupEdge = (
    event: DragEvent<HTMLElement>,
    groupId: string,
    parentId: string | null,
    edge: "before" | "after",
  ): void => {
    if (draggedGroup === null || !canMoveGroup(groups, draggedGroup, parentId)) {
      event.stopPropagation();
      event.dataTransfer.dropEffect = "none";
      return;
    }
    const siblings = groups.groups.filter((group) => (group.parentId ?? null) === parentId && group.id !== draggedGroup);
    const index = siblings.findIndex((group) => group.id === groupId);
    if (index < 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    setZone(null);
    setCaret({ parentId, index: index + (edge === "after" ? 1 : 0) });
  };

  const overGroupHeader = (
    event: DragEvent<HTMLElement>,
    groupId: string,
    parentId: string | null,
  ): void => {
    const edge = draggedProjectKind(event.dataTransfer.types) === "group" ? headerEdge(event) : null;
    if (edge !== null) {
      overGroupEdge(event, groupId, parentId, edge);
      return;
    }
    over(event, groupId);
  };

  const dropGroupHeader = (
    event: DragEvent<HTMLElement>,
    groupId: string,
    parentId: string | null,
  ): void => {
    if (event.dataTransfer.getData(PROJECT_GROUP_DRAG_MIME) !== "" && headerEdge(event) !== null) {
      dropGroups(event, parentId);
      return;
    }
    drop(event, groupId);
  };

  return {
    containerRef,
    highlights: (target: string | null): boolean => zone?.id === target,
    isCaretBefore: (parentId: string | null, groupId: string): boolean => {
      if (caret?.parentId !== parentId) return false;
      const siblings = groups.groups.filter((group) => (group.parentId ?? null) === parentId && group.id !== draggedGroup);
      return siblings[caret.index]?.id === groupId;
    },
    isCaretAtEnd: (parentId: string | null): boolean => {
      if (caret?.parentId !== parentId) return false;
      const siblings = groups.groups.filter((group) => (group.parentId ?? null) === parentId && group.id !== draggedGroup);
      return caret.index === siblings.length;
    },
    dragging: dragged !== null || draggedGroup !== null,
    onDragProject: (repo: string | null): void => {
      setDragged(repo);
      if (repo === null) clear();
    },
    onDragStartGroup: (event: DragEvent<HTMLElement>, id: string): void => {
      event.dataTransfer.setData(PROJECT_GROUP_DRAG_MIME, id);
      event.dataTransfer.effectAllowed = "move";
      setDraggedGroup(id);
    },
    onDragEnd: clear,
    onDragOver: over,
    onDragOverGroupHeader: overGroupHeader,
    onDragOverGroups: overGroups,
    onDragLeave: (event: DragEvent<HTMLElement>): void => {
      if (!stillInside(event)) setZone(null);
    },
    onDragLeaveGroups: (event: DragEvent<HTMLElement>): void => {
      if (!stillInside(event)) setCaret(null);
    },
    onDrop: drop,
    onDropGroupHeader: dropGroupHeader,
    onDropGroups: dropGroups,
  };
}
