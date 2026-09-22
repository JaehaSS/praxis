import type { DiffHunk, RematchedAnnotation } from "../lib/ipc";
import {
  annotationLine,
  groupAnnotationsByPosition,
  orphanedAnnotations,
  positionKey,
} from "../lib/annotations";
import { intraLineSegments, toSplitRowsFromHunks, type Segment, type SplitCell } from "../lib/diff";
import {
  AnnotationComposer,
  AnnotationThread,
  OrphanedAnnotationsNotice,
  useAnnotationComposer,
  type AnnotationComposerState,
} from "./DiffAnnotationThread";
import { DiffLineText, HunkCheckbox } from "./DiffPresentation";

const cellTone: Record<string, string> = {
  add: "bg-addbg text-status-done",
  del: "bg-delbg text-status-failed",
  context: "text-text-secondary",
};

interface Props {
  hunks: DiffHunk[];
  annotations: RematchedAnnotation[];
  onCreate: (input: { hunk_id: string; line: number; side: string; body_md: string }) => Promise<void>;
  onUpdateBody: (id: string, body_md: string) => Promise<void>;
  /** hunk 부분 승인(B-2) 선택 상태 — 생략 시 체크박스를 표시하지 않는다. */
  selectedHunkIds?: Set<string>;
  onToggleHunk?: (hunk: DiffHunk) => void;
}

/** 좌우 셀 하나. 폭을 정확히 반으로 고정하고 줄바꿈을 막는다 — 래핑을 허용하면 한쪽만
 *  높아져 좌우 정렬이 깨지고, 비교라는 목적 자체가 무너진다. 잘린 줄은 title로 볼 수 있다. */
function SplitCellView({
  cell,
  side,
  hasThread,
  segments,
  onAnnotate,
}: {
  cell: SplitCell | null;
  /** 어느 열인지 — context 행은 좌우가 같은 cell을 공유하므로 이걸로만 번호를 가른다. */
  side: "left" | "right";
  hasThread: boolean;
  segments?: Segment[];
  onAnnotate: () => void;
}) {
  if (!cell) return <div className="w-1/2 shrink-0 bg-empty" />;
  const { line } = cell;
  const lineNumber = side === "left" ? (line.oldLine ?? line.newLine) : (line.newLine ?? line.oldLine);
  return (
    <div className={`group/cell w-1/2 shrink-0 flex overflow-hidden ${cellTone[line.kind] ?? ""}`}>
      <button
        className={`w-4 shrink-0 text-[10px] leading-5 ${
          hasThread ? "opacity-100" : "opacity-0 group-hover/cell:opacity-100"
        }`}
        title="주석 남기기"
        onClick={onAnnotate}
      >
        💬
      </button>
      <span className="w-10 shrink-0 text-right pr-2 text-text-muted select-none">
        {lineNumber ?? ""}
      </span>
      <DiffLineText
        text={line.text || " "}
        segments={segments}
        kind={line.kind}
        className="whitespace-pre"
      />
    </div>
  );
}

/** 한 행에 달린 주석 스레드와 작성기 — 좌우 어느 쪽에 걸렸든 행 아래 전체 폭에 렌더한다.
 *  좁은 셀 안에 마크다운을 넣으면 좌우 행 높이가 어긋나기 때문이다. */
function RowThreads({
  cells,
  grouped,
  composer,
}: {
  cells: SplitCell[];
  grouped: Map<string, RematchedAnnotation[]>;
  composer: AnnotationComposerState;
}) {
  return (
    <>
      {cells.map((cell) => {
        const key = positionKey(cell.hunkId, annotationLine(cell.line), cell.line.side);
        return (
          <div key={key}>
            <AnnotationThread
              thread={grouped.get(key) ?? []}
              indent="ml-14"
              onEdit={(a) =>
                composer.openEdit(cell.hunkId, annotationLine(cell.line), cell.line.side, a)
              }
            />
            {composer.activeKey === key && (
              <AnnotationComposer
                indent="ml-14"
                value={composer.text}
                error={composer.error}
                onChange={composer.setText}
                onSave={composer.save}
              />
            )}
          </div>
        );
      })}
    </>
  );
}

/** split 모드 diff — 좌우 비교를 유지하면서 통합 모드와 같은 주석·부분 승인을 제공한다.
 *  통합 모드와 동일한 DiffHunk[]를 입력으로 받으므로 주석 좌표(hunk id·side)가 어긋나지 않는다. */
export function AnnotatedSplitDiff({
  hunks,
  annotations,
  onCreate,
  onUpdateBody,
  selectedHunkIds,
  onToggleHunk,
}: Props) {
  const composer = useAnnotationComposer(onCreate, onUpdateBody);
  const grouped = groupAnnotationsByPosition(annotations);
  const rows = toSplitRowsFromHunks(hunks);

  return (
    <div className="text-xs font-code leading-relaxed">
      <OrphanedAnnotationsNotice orphaned={orphanedAnnotations(annotations)} />
      {rows.map((row, index) => {
        if (row.kind === "gap") {
          return (
            <div key={index} className="my-1 px-3 py-1 text-center bg-raised text-text-muted">
              {row.text}
            </div>
          );
        }
        if (row.kind === "hunk") {
          return (
            <div
              key={index}
              data-hunk-id={row.hunk.id}
              className="px-3 flex items-center gap-2 text-primary-bright bg-empty"
            >
              <HunkCheckbox hunk={row.hunk} selectedHunkIds={selectedHunkIds} onToggle={onToggleHunk} />
              <span>{row.text}</span>
            </div>
          );
        }
        // context 행은 좌우가 같은 객체다 — 스레드를 두 번 렌더하지 않도록 한 칸만 넘긴다.
        const cells = (row.left === row.right ? [row.left] : [row.left, row.right]).filter(
          (cell): cell is SplitCell => cell !== null,
        );
        // 좌우가 del/add 쌍일 때만 단어 단위 강조를 계산한다.
        const intra =
          row.left && row.right && row.left !== row.right
            ? intraLineSegments(row.left.line.text, row.right.line.text)
            : null;
        return (
          <div key={index}>
            <div className="flex hover:bg-border/40">
              <SplitCellView
                cell={row.left}
                side="left"
                hasThread={threadAt(grouped, row.left).length > 0}
                segments={intra?.before}
                onAnnotate={() => openAt(composer, row.left)}
              />
              <div className="w-px bg-border shrink-0" />
              <SplitCellView
                cell={row.right}
                side="right"
                hasThread={threadAt(grouped, row.right).length > 0}
                segments={intra?.after}
                onAnnotate={() => openAt(composer, row.right)}
              />
            </div>
            <RowThreads cells={cells} grouped={grouped} composer={composer} />
          </div>
        );
      })}
    </div>
  );
}

function threadAt(
  grouped: Map<string, RematchedAnnotation[]>,
  cell: SplitCell | null,
): RematchedAnnotation[] {
  if (!cell) return [];
  return grouped.get(positionKey(cell.hunkId, annotationLine(cell.line), cell.line.side)) ?? [];
}

function openAt(composer: AnnotationComposerState, cell: SplitCell | null): void {
  if (!cell) return;
  composer.openNew(cell.hunkId, annotationLine(cell.line), cell.line.side);
}
