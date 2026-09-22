/** 사이드바 프로젝트 목록의 그룹 트리와 마지막 미소속 구획. */

import { Fragment, type ReactNode } from "react";
import {
  assignProject,
  dissolveGroup,
  renameGroup,
  toggleGroup,
  type ProjectGroups,
} from "../../lib/project-groups";
import type { ProjectSection } from "../../lib/project-group-tree";
import { groupPath } from "../../lib/project-group-tree";
import { DROP_CARET_CLASS, DROP_ZONE_CLASS } from "./project-drag";
import { SessionTaskGroup } from "./SessionTaskGroup";
import type { TaskNavigationMenuState } from "./TaskNavigationMenu";
import type { ProjectGroupDrag } from "./useProjectGroupDrag";

export type RenamingGroup =
  | { id: string; created: false }
  | { id: string; created: true; repo: string; previousGroupId: string | null }
  | { id: string; created: true; repo: null }
  | null;

export interface SessionTaskSectionsProps {
  grouped: ProjectSection[];
  unassigned: string[];
  groups: ProjectGroups;
  drag: ProjectGroupDrag;
  renaming: RenamingGroup;
  onChange: (next: ProjectGroups) => void;
  onRenaming: (next: RenamingGroup) => void;
  onOpenMenu: (menu: TaskNavigationMenuState) => void;
  renderProject: (repo: string) => ReactNode;
}

const Caret = () => <div data-drop-caret className={`${DROP_CARET_CLASS} pointer-events-none absolute inset-x-2 bottom-0`} />;

export function SessionTaskSections(props: SessionTaskSectionsProps) {
  const { drag, groups, grouped, renaming, unassigned } = props;
  const cancelRename = (id: string): void => {
    props.onRenaming(null);
    if (renaming?.created !== true) return;
    const next = dissolveGroup(groups, id);
    // 프로젝트 없이 만든 그룹은 되돌릴 소속이 없다 — 그룹만 사라진다.
    if (renaming.repo === null || renaming.previousGroupId === null) {
      props.onChange(next);
      return;
    }
    props.onChange(assignProject(next, renaming.repo, renaming.previousGroupId));
  };
  const commitRename = (id: string, name: string): void => {
    props.onRenaming(null);
    props.onChange(renameGroup(groups, id, name));
  };
  const project = (repo: string, depth: number) => (
    <div key={repo} className="min-w-0" style={{ marginLeft: `${(Math.min(depth + 1, 3) - Math.min(depth, 3)) * 8}px` }}>
      {props.renderProject(repo)}
    </div>
  );
  const renderGroups = (sections: ProjectSection[], parentId: string | null, depth: number) => (
    <div
      data-group-list
      className="relative flex flex-col gap-1.5"
      onDragOver={(event) => drag.onDragOverGroups(event, parentId)}
      onDragLeave={drag.onDragLeaveGroups}
      onDrop={(event) => drag.onDropGroups(event, parentId)}
    >
      {sections.map((section) => {
        const group = section.group!;
        return (
          <Fragment key={group.id}>
            <SessionTaskGroup
              group={group}
              count={section.repos.length}
              hasChildren={section.children.length > 0}
              depth={depth}
              path={groupPath(groups.groups, group.id)}
              caretBefore={drag.isCaretBefore(parentId, group.id)}
              renaming={renaming?.id === group.id}
              highlighted={drag.highlights(group.id)}
              onToggle={() => props.onChange(toggleGroup(groups, group.id))}
              onRename={(name) => commitRename(group.id, name)}
              onCancelRename={() => cancelRename(group.id)}
              onOpenMenu={props.onOpenMenu}
              onDragStart={(event) => drag.onDragStartGroup(event, group.id)}
              onDragEnd={drag.onDragEnd}
              onDragOver={(event) => drag.onDragOver(event, group.id)}
              onHeaderDragOver={(event) => drag.onDragOverGroupHeader(event, group.id, parentId)}
              onDragLeave={drag.onDragLeave}
              onDrop={(event) => drag.onDrop(event, group.id)}
              onHeaderDrop={(event) => drag.onDropGroupHeader(event, group.id, parentId)}
            >
              {renderGroups(section.children, group.id, depth + 1)}
              {section.repos.map((repo) => project(repo, depth))}
            </SessionTaskGroup>
          </Fragment>
        );
      })}
      {drag.isCaretAtEnd(parentId) && <Caret />}
    </div>
  );

  if (grouped.length === 0) return <>{unassigned.map(props.renderProject)}</>;

  return (
    <>
      <div ref={drag.containerRef}>{renderGroups(grouped, null, 0)}</div>
      <div
        data-unassigned
        className={`flex flex-col gap-1.5 rounded-lg border-2 ${drag.highlights(null) ? DROP_ZONE_CLASS : "border-transparent"}`}
        onDragOver={(event) => drag.onDragOver(event, null)}
        onDragLeave={drag.onDragLeave}
        onDrop={(event) => drag.onDrop(event, null)}
      >
        {unassigned.map(props.renderProject)}
        {unassigned.length === 0 && drag.dragging && (
          <div className="rounded-lg border border-dashed border-border-strong px-2.5 py-2 text-center text-[11px] text-text-muted">
            그룹에서 빼기
          </div>
        )}
      </div>
    </>
  );
}
