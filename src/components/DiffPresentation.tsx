import type { DiffHunk, FileDiff } from "../lib/ipc";
import type { DiffRange } from "../lib/transport";
import { describePatch, summarizePatch, toSplitRows, type Segment, type Side } from "../lib/diff";

export type DiffMode = "unified" | "split";

const sideBg: Record<Side["kind"], string> = {
  ctx: "",
  add: "bg-addbg text-status-done",
  del: "bg-delbg text-status-failed",
  empty: "bg-empty",
  hunk: "text-primary-bright",
  gap: "bg-raised text-text-muted",
};

function SideCell({ side }: { side: Side }) {
  return (
    <div className={`flex flex-1 min-w-0 ${sideBg[side.kind]}`}>
      <span className="w-10 shrink-0 text-right pr-2 text-text-muted select-none">
        {side.n ?? ""}
      </span>
      <span className="whitespace-pre-wrap break-all">{side.text || " "}</span>
    </div>
  );
}

/** diff 한 줄의 본문 — 단어 단위 강조가 있으면 바뀐 구간만 2단계 톤으로 덧칠한다.
 *  segments가 null이면(통째로 바뀐 줄·컨텍스트 줄) 줄 틴트만 남기고 평문으로 렌더한다. */
export function DiffLineText({
  text,
  segments,
  kind,
  className,
}: {
  text: string;
  segments?: Segment[];
  kind: string;
  className?: string;
}) {
  if (!segments) return <span className={className}>{text}</span>;
  const strong = kind === "add" ? "bg-addbg-strong" : "bg-delbg-strong";
  return (
    <span className={className}>
      {segments.map((segment, index) => (
        <span key={index} className={segment.changed ? strong : undefined}>
          {segment.text}
        </span>
      ))}
    </span>
  );
}

/** hunk 부분 승인(B-2) 선택 체크박스 — 통합·분할 모드가 공유한다.
 *  `selectedHunkIds`가 없으면(부분 승인 비활성) 아무것도 렌더하지 않는다. */
export function HunkCheckbox({
  hunk,
  selectedHunkIds,
  onToggle,
}: {
  hunk: DiffHunk;
  selectedHunkIds?: Set<string>;
  onToggle?: (hunk: DiffHunk) => void;
}) {
  if (!selectedHunkIds) return null;
  // 커밋된 hunk는 고를 것이 없다 — 유지도 폐기도 대상이 아니라 비활성 체크박스조차
  // 오해를 부른다(protected는 "체크 안 하면 지워짐"이라 비활성으로 남기는 것이 맞다).
  if (hunk.committed) {
    return (
      <span
        className="rounded bg-raised px-1 text-[10px] text-text-muted"
        title="이미 커밋된 변경 — 부분 적용이 건드리지 않습니다"
      >
        커밋됨
      </span>
    );
  }
  return (
    <input
      type="checkbox"
      checked={!hunk.protected && selectedHunkIds.has(hunk.id)}
      disabled={hunk.protected}
      title={
        hunk.protected
          ? "protected 경로 변경 — 부분 적용으로 유지할 수 없습니다(항상 제거됨)"
          : undefined
      }
      onChange={() => onToggle?.(hunk)}
    />
  );
}

export function DiffRangeToggle({
  range,
  onChange,
}: {
  range: DiffRange;
  onChange: (range: DiffRange) => void;
}) {
  const options: { value: DiffRange; label: string; title: string }[] = [
    { value: "session", label: "세션 전체", title: "이 작업이 시작된 뒤의 모든 변경 (커밋 포함)" },
    { value: "uncommitted", label: "미커밋", title: "아직 커밋하지 않은 변경만" },
  ];
  return (
    <div
      className="flex h-7 rounded-md border border-border bg-surface p-0.5"
      role="group"
      aria-label="Diff 범위"
    >
      {options.map((option) => (
        <button
          key={option.value}
          className={`rounded px-2.5 text-xs ${
            range === option.value
              ? "bg-raised text-text shadow-sm"
              : "text-text-muted hover:text-text-secondary"
          }`}
          aria-pressed={range === option.value}
          title={option.title}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function DiffModeToggle({
  mode,
  onChange,
  disabled = false,
  disabledTitle,
}: {
  mode: DiffMode;
  onChange: (mode: DiffMode) => void;
  /** 본문이 좁아 unified가 강제된 구간 — 고를 수 있는 척하지 않는다(설계 DR-6). */
  disabled?: boolean;
  disabledTitle?: string;
}) {
  const options: { value: DiffMode; label: string; title: string }[] = [
    { value: "unified", label: "통합", title: "한 열에서 추가·삭제 함께 보기" },
    { value: "split", label: "분할", title: "이전·이후를 두 열로 비교하기" },
  ];
  return (
    <div className="flex h-7 rounded-md border border-border bg-surface p-0.5" role="group" aria-label="Diff 보기 방식">
      {options.map((option) => (
        <button
          key={option.value}
          className={`rounded px-2.5 text-xs disabled:opacity-50 ${
            mode === option.value
              ? "bg-raised text-text shadow-sm"
              : "text-text-muted hover:text-text-secondary"
          }`}
          aria-pressed={mode === option.value}
          disabled={disabled}
          title={disabled ? (disabledTitle ?? option.title) : option.title}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function DiffFileHeader({ file }: { file: FileDiff }) {
  const summary = summarizePatch(file.patch);
  return (
    <div className="min-h-9 shrink-0 border-b border-border bg-raised px-3 py-2 flex items-center gap-2 font-code text-xs">
      <span className="text-text-muted">{file.status}</span>
      <span className="min-w-0 flex-1 truncate text-text" title={file.path}>
        {file.path}
      </span>
      <span className="text-status-done">+{summary.additions}</span>
      <span className="text-status-failed">−{summary.deletions}</span>
    </div>
  );
}

export function PatchFallback({ patch }: { patch: string }) {
  return (
    <div className="m-3 rounded-md border border-border bg-surface px-3 py-4 text-sm text-text-muted">
      {describePatch(patch)}
    </div>
  );
}

export function SplitDiff({ patch }: { patch: string }) {
  const rows = toSplitRows(patch);
  const contentRows = rows.filter((row) => !["hunk", "gap"].includes(row.left.kind));
  if (contentRows.length === 0) return <PatchFallback patch={patch} />;
  return (
    <div className="text-xs font-code leading-relaxed min-w-[720px]">
      {rows.map((row, index) => {
        if (row.left.kind === "hunk") {
          return (
            <div key={index} className="px-3 text-primary-bright bg-empty">
              {row.left.text}
            </div>
          );
        }
        if (row.left.kind === "gap") {
          return (
            <div key={index} className="my-1 px-3 py-1 text-center bg-raised text-text-muted">
              {row.left.text}
            </div>
          );
        }
        return (
          <div key={index} className="flex">
            <SideCell side={row.left} />
            <div className="w-px bg-border shrink-0" />
            <SideCell side={row.right} />
          </div>
        );
      })}
    </div>
  );
}
