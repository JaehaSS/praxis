import { useRef, type DragEvent, type KeyboardEvent, type ReactNode } from "react";
import { getProjectGroupColor } from "../../lib/project-group-colors";
import type { ProjectGroup } from "../../lib/project-groups";
import { Icon } from "./icons";
import { DROP_ZONE_CLASS } from "./project-drag";
import type { TaskNavigationMenuState } from "./TaskNavigationMenu";

export interface SessionTaskGroupProps {
  group: ProjectGroup;
  count: number;
  hasChildren: boolean;
  depth: number;
  path: string;
  caretBefore: boolean;
  renaming: boolean;
  highlighted: boolean;
  onToggle: () => void;
  onRename: (name: string) => void;
  onCancelRename: () => void;
  onOpenMenu: (menu: TaskNavigationMenuState) => void;
  onDragStart: (event: DragEvent<HTMLElement>) => void;
  onDragEnd: () => void;
  onDragOver: (event: DragEvent<HTMLElement>) => void;
  onHeaderDragOver: (event: DragEvent<HTMLElement>) => void;
  onDragLeave: (event: DragEvent<HTMLElement>) => void;
  onDrop: (event: DragEvent<HTMLElement>) => void;
  onHeaderDrop: (event: DragEvent<HTMLElement>) => void;
  children: ReactNode;
}

function GroupNameInput({ name, onCommit, onCancel }: {
  name: string;
  onCommit: (next: string) => void;
  onCancel: () => void;
}) {
  const settled = useRef(false);
  const finish = (value: string): void => {
    if (settled.current) return;
    settled.current = true;
    const trimmed = value.trim();
    if (trimmed === "") onCancel();
    else onCommit(trimmed);
  };
  return (
    <input
      autoFocus
      defaultValue={name}
      aria-label="그룹 이름"
      className="h-6 min-w-0 flex-1 rounded border border-border-strong bg-surface px-1 text-xs font-medium normal-case tracking-normal text-text"
      onClick={(event) => event.stopPropagation()}
      onMouseDown={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.nativeEvent.isComposing || event.keyCode === 229) return;
        if (event.key === "Enter") finish(event.currentTarget.value);
        else if (event.key === "Escape") {
          settled.current = true;
          onCancel();
        }
      }}
      onBlur={(event) => finish(event.currentTarget.value)}
    />
  );
}

export function SessionTaskGroup(props: SessionTaskGroupProps) {
  const { group, renaming } = props;
  const color = getProjectGroupColor(group.color);
  const indent = `${(Math.min(props.depth, 3) - Math.min(Math.max(props.depth - 1, 0), 3)) * 8}px`;
  const border = props.depth < 3 ? "border-2" : "border-0";
  const headerStyle = props.highlighted || color === undefined ? undefined : { backgroundColor: color.tint };
  const openMenu = (header: HTMLElement, x: number, y: number): void => {
    props.onOpenMenu({ x, y, kind: "group", groupId: group.id, trigger: header });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (renaming) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      props.onToggle();
      return;
    }
    if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
    event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect();
    openMenu(event.currentTarget, rect.left, rect.bottom);
  };
  return (
    <div
      data-group-box={group.id}
      className={`relative min-w-0 rounded-lg ${border} transition-colors ${props.highlighted ? DROP_ZONE_CLASS : "border-transparent"}`}
      style={{ marginLeft: indent }}
      onDragOver={props.onDragOver}
      onDragLeave={props.onDragLeave}
      onDrop={props.onDrop}
    >
      {!props.highlighted && color && (
        <div
          aria-hidden
          data-group-color-strip={color.id}
          className="pointer-events-none absolute inset-y-[2px] left-[2px] w-[3px] rounded-l-md"
          style={{ backgroundColor: color.accent }}
        />
      )}
      {props.caretBefore && <div data-drop-caret className="pointer-events-none absolute inset-x-2 top-0 h-0.5 rounded bg-primary" />}
      <div
        data-group-header={group.id}
        data-group-color={color?.id}
        className="flex items-center justify-between gap-1.5 px-2.5 py-1 text-xs font-semibold uppercase tracking-[0.08em] text-text cursor-pointer hover:bg-raised/60 transition-colors"
        style={headerStyle}
        draggable={!renaming}
        role="button"
        tabIndex={renaming ? -1 : 0}
        aria-expanded={!group.collapsed}
        aria-haspopup="menu"
        title={props.path}
        onDragStart={props.onDragStart}
        onDragEnd={props.onDragEnd}
        onDragOver={props.onHeaderDragOver}
        onDrop={props.onHeaderDrop}
        onKeyDown={onKeyDown}
        onClick={() => {
          if (!renaming) props.onToggle();
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          openMenu(event.currentTarget, event.clientX, event.clientY);
        }}
      >
        {renaming ? (
          <GroupNameInput name={group.name} onCommit={props.onRename} onCancel={props.onCancelRename} />
        ) : (
          <span className="flex items-center gap-1.5 truncate" title={props.path}>
            <Icon name={group.collapsed ? "chevronRight" : "chevronDown"} size={12} />
            <span className="truncate">{group.name}</span>
          </span>
        )}
        <span className="shrink-0 font-code">{props.count}</span>
      </div>
      {!group.collapsed && (
        <div data-group-body className="flex flex-col gap-1.5">
          {!props.hasChildren && props.count === 0 ? (
            <div className="px-2.5 pb-1 text-[11px] text-text-muted">프로젝트 없음</div>
          ) : props.children}
        </div>
      )}
    </div>
  );
}
