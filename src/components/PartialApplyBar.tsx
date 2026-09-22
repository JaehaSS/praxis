import type { PartialApplyState } from "./use-partial-apply";

/** hunk 부분 승인(B-2) 액션바 — 선택 적용 → 확인 스텝 → 결과/롤백. unified 모드 전용. */
export function PartialApplyBar({ state }: { state: PartialApplyState }) {
  const { keptCount, discardedCount, confirming, busy, result, error } = state;

  if (confirming) {
    return (
      <div className="shrink-0 border-b border-border bg-raised px-3 py-1.5 flex items-center gap-3 text-xs font-ui">
        <span className="text-text-secondary">
          선택 {keptCount}개 적용, 비선택 {discardedCount}개 hunk는 제거됩니다 (적용 후 되돌리기 가능).
        </span>
        <button
          className="h-6 px-2 rounded bg-primary text-bg disabled:opacity-50 shrink-0"
          disabled={busy}
          onClick={() => void state.apply()}
        >
          {busy ? "적용 중…" : "적용"}
        </button>
        <button
          className="h-6 px-2 rounded text-text-secondary hover:bg-border shrink-0"
          disabled={busy}
          onClick={state.cancelConfirm}
        >
          취소
        </button>
      </div>
    );
  }

  return (
    <div className="shrink-0 border-b border-border px-3 py-1.5 flex items-center gap-3 text-xs font-ui">
      <button
        className="h-6 px-2 rounded text-text bg-raised border border-border hover:border-primary-bright disabled:opacity-50 shrink-0"
        disabled={busy || discardedCount === 0}
        onClick={state.requestConfirm}
      >
        선택 적용 ({keptCount} hunks)
      </button>
      {error && <span className="text-status-failed">{error}</span>}
      {result && (
        <span className="flex items-center gap-2 text-status-done">
          적용 완료 — 유지 {result.kept_hunk_ids.length} / 제거 {result.discarded_hunk_ids.length}
          <button
            className="h-6 px-2 rounded bg-status-failed/15 border border-dangerborder text-status-failed disabled:opacity-50"
            disabled={busy}
            onClick={() => void state.rollback()}
          >
            롤백
          </button>
        </span>
      )}
    </div>
  );
}
