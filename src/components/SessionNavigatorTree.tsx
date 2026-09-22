import type { KeyboardEvent, MutableRefObject } from "react";
import { taskStatusLabel } from "../lib/task-status";
import type { SessionNavigatorRow } from "../lib/session-navigator";
import { Icon } from "./ide/icons";

interface Props {
  rows: SessionNavigatorRow[];
  activeId: string | null;
  expanded: (row: SessionNavigatorRow) => boolean;
  rowRefs: MutableRefObject<Map<string, HTMLElement>>;
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>, row: SessionNavigatorRow) => void;
  onClick: (row: SessionNavigatorRow) => void;
}

const isBranch = (row: SessionNavigatorRow): boolean =>
  row.kind === "group" || row.kind === "project";

const projectPath = (row: SessionNavigatorRow): string => row.id.slice("project:".length);

const sessionDetail = (row: SessionNavigatorRow): string =>
  `${row.task!.host} · ${row.task!.stale ? "오프라인" : taskStatusLabel(row.task!)}`;

function TreeRow({ row, activeId, expanded, rowRefs, onKeyDown, onClick }: Props & { row: SessionNavigatorRow }) {
  const selected = row.id === activeId;
  const expandable = isBranch(row);
  return (
    <button
      id={`session-navigator-${row.id}`}
      ref={(element) => {
        if (element) rowRefs.current.set(row.id, element);
        else rowRefs.current.delete(row.id);
      }}
      role="treeitem"
      aria-level={row.depth + 1}
      aria-expanded={expandable ? expanded(row) : undefined}
      aria-disabled={row.kind === "empty" || undefined}
      disabled={row.kind === "empty"}
      tabIndex={selected ? 0 : -1}
      className={`flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-sm ${selected ? "bg-primary/10 text-primary-bright" : "text-text-secondary hover:bg-surface hover:text-text"} ${row.kind === "empty" ? "cursor-default text-text-muted" : ""}`}
      style={{ paddingLeft: `${10 + row.depth * 18}px` }}
      onKeyDown={(event) => onKeyDown(event, row)}
      onClick={() => onClick(row)}
    >
      {expandable ? <span className="w-4 text-text-muted"><Icon name={expanded(row) ? "chevronDown" : "chevronRight"} size={14} /></span> : <span className="w-4" aria-hidden="true" />}
      <span className="min-w-0 flex-1 truncate" title={row.kind === "project" ? projectPath(row) : undefined}>{row.label}</span>
      {row.kind === "project" && <span className="max-w-[45%] truncate text-xs text-text-muted">{projectPath(row)}</span>}
      {row.task && <span className="shrink-0 text-xs text-text-muted">{sessionDetail(row)}</span>}
    </button>
  );
}

export function SessionNavigatorTree(props: Props) {
  return (
    <div id="session-navigator-tree" className="overflow-auto p-1" role="tree" aria-label="그룹과 세션">
      {props.rows.length === 0 ? <div className="px-3 py-6 text-center text-sm text-text-muted">일치하는 항목이 없습니다</div> : props.rows.map((row) => <TreeRow key={row.id} row={row} {...props} />)}
    </div>
  );
}
