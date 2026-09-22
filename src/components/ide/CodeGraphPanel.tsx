import type {
  CodeGraphImpact,
  CodeGraphIncompleteness,
  CodeGraphStatus,
  ImpactedSymbol,
} from "../../lib/ipc";

const BUILD_LABEL: Record<CodeGraphStatus["buildState"], string> = {
  idle: "",
  indexing_symbols: "심볼 수집",
  waiting_semantic: "언어 분석 대기",
  indexing_edges: "참조 수집",
  degraded: "일부",
  failed: "실패",
  cancelled: "취소됨",
};

function statusLabel(status: CodeGraphStatus | null, dirty: boolean, busy: boolean): string {
  if (status == null) return busy ? "심볼 수집" : "확인 중";
  if (["indexing_symbols", "waiting_semantic", "indexing_edges"].includes(status.buildState)) {
    return BUILD_LABEL[status.buildState];
  }
  const active = status.activeState === "ready" && !dirty ? "준비됨" : "오래됨";
  if (status.activeState !== "absent" && status.buildState !== "idle") {
    return `${active} · 새 빌드 ${BUILD_LABEL[status.buildState]}`;
  }
  if (status.activeState !== "absent") return active;
  return BUILD_LABEL[status.buildState] || "없음";
}

/** "왜 이 언어는 참조가 안 나오나"에 답한다 — 신선도 라벨 옆에 따로 선다. */
function incompleteLabel(incomplete: CodeGraphIncompleteness): string {
  const languages = incomplete.languagesWithoutEdges.join(", ");
  if (incomplete.filesWithoutEdges > 0) {
    return `${languages} 참조 분석 없음 (${incomplete.filesWithoutEdges}개 파일)`;
  }
  return `${incomplete.filesSkipped}개 파일 건너뜀`;
}

interface ControlProps {
  status: CodeGraphStatus | null;
  dirty: boolean;
  busy: boolean;
  onIndex: () => void;
  onCancel: () => void;
  onImpact: () => void;
  onNeighborhood?: () => void;
}

export function CodeGraphControl({
  status,
  dirty,
  busy,
  onIndex,
  onCancel,
  onImpact,
  onNeighborhood,
}: ControlProps) {
  const active = status?.activeState !== "absent" && status?.activeRunId != null;
  const running =
    busy ||
    status?.buildState === "indexing_symbols" ||
    status?.buildState === "waiting_semantic" ||
    status?.buildState === "indexing_edges";
  const detail = status?.detail ?? undefined;
  const incomplete = status?.incomplete ?? null;
  return (
    <div className="flex items-center gap-1 text-xs" title={detail}>
      <span className={active ? "text-text-muted" : "text-status-awaiting"}>
        코드 그래프: {statusLabel(status, dirty, busy)}
      </span>
      {incomplete && (
        <span className="text-status-awaiting" title={incomplete.detail} role="status">
          · {incompleteLabel(incomplete)}
        </span>
      )}
      {detail && (
        <span className="max-w-64 break-words text-status-failed" role="status">
          {detail}
        </span>
      )}
      {running ? (
        <button
          className="h-6 px-1.5 text-status-awaiting hover:text-text"
          onClick={onCancel}
          aria-label="코드 그래프 인덱싱 취소"
        >
          취소
        </button>
      ) : (
        <button
          className="h-6 px-1.5 text-text-muted hover:text-text"
          onClick={onIndex}
          aria-label={active ? "코드 그래프 새로 고침" : "코드 그래프 만들기"}
        >
          {active ? "새로 고침" : "그래프 만들기"}
        </button>
      )}
      <button
        className="h-6 px-1.5 rounded border border-border text-text-secondary hover:text-text disabled:opacity-40"
        onClick={onImpact}
        disabled={!active || running || dirty}
        aria-label="영향 범위 보기"
      >
        영향 범위
      </button>
      {onNeighborhood && (
        <button
          className="h-6 rounded border border-border px-1.5 text-text-secondary hover:text-text disabled:opacity-40"
          onClick={onNeighborhood}
          disabled={!active || running || dirty}
          aria-label="참조 그래프 보기"
        >
          참조 그래프
        </button>
      )}
    </div>
  );
}

interface PanelProps {
  embedded?: boolean;
  impact: CodeGraphImpact | null;
  error: string | null;
  onOpen: (item: ImpactedSymbol) => void;
  onClose: () => void;
}

function ImpactGroup({
  title,
  items,
  onOpen,
}: {
  title: string;
  items: ImpactedSymbol[];
  onOpen: (item: ImpactedSymbol) => void;
}) {
  if (items.length === 0) return null;
  return (
    <section className="space-y-1">
      <h3 className="text-xs text-text-muted">{title} · {items.length}</h3>
      {items.map((item) => (
        <button
          key={item.id}
          data-impact-id={item.id}
          className="block w-full rounded px-2 py-1 text-left hover:bg-bg"
          onClick={() => onOpen(item)}
        >
          <span className="block truncate text-sm text-text">{item.name}</span>
          <span className="block truncate font-code text-xs text-text-muted">
            {item.relPath}:{item.line + 1}
            {item.container ? ` · ${item.container}` : ""}
          </span>
        </button>
      ))}
    </section>
  );
}

export function CodeGraphPanel({ embedded = false, impact, error, onOpen, onClose }: PanelProps) {
  const direct = impact?.items.filter((item) => item.depth === 1) ?? [];
  const indirect = impact?.items.filter((item) => item.depth > 1) ?? [];
  return (
    <aside className={`bg-raised p-3 ${embedded ? "min-h-0 min-w-0 flex-1 overflow-auto" : "absolute right-2 top-10 z-30 w-96 max-w-[calc(100%-1rem)] rounded-lg border border-border-strong shadow-xl"}`}>
      <div className="mb-2 flex items-center gap-2">
        <h2 className="text-sm text-text">영향 범위 · {impact?.items.length ?? 0}</h2>
        {impact?.freshness === "stale" && (
          <span className="text-xs text-status-awaiting">오래된 인덱스</span>
        )}
        {impact?.truncated && <span className="text-xs text-status-awaiting">500건에서 잘림</span>}
        <button className="ml-auto text-text-muted hover:text-text" onClick={onClose} aria-label="영향 범위 닫기">
          ×
        </button>
      </div>
      {error && <div className="text-xs text-status-failed" role="alert">{error}</div>}
      {!error && impact?.edgesUnavailable && (
        <div className="text-xs text-status-awaiting" role="status">
          이 파일은 참조를 분석하지 못했습니다 — 영향이 없다는 뜻이 아닙니다. {impact.edgesUnavailable}
        </div>
      )}
      {!error && !impact?.edgesUnavailable && impact?.items.length === 0 && (
        <div className="text-xs text-text-muted">이 심볼을 참조하는 인덱싱된 심볼이 없습니다.</div>
      )}
      {!error && (
        <div className="max-h-80 space-y-3 overflow-auto">
          <ImpactGroup title="직접 영향" items={direct} onOpen={onOpen} />
          <ImpactGroup title="간접 영향" items={indirect} onOpen={onOpen} />
        </div>
      )}
    </aside>
  );
}
