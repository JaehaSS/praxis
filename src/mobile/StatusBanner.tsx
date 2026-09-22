import { describeConnection, type ConnectionState } from "./status";
import { StatusPill } from "./primitives";

// 상태 배너 — 이 앱에서 가장 중요한 한 줄. (설계 0013 §7.2)
// "연결 안 됨"으로 뭉치지 않고 원인을 나눠 보여주는 것이 존재 이유다.

export function StatusBanner({
  state,
  onRetry,
  nowSecs,
}: {
  state: ConnectionState;
  onRetry: () => void;
  /** 테스트·렌더 일관성을 위해 주입. 생략하면 현재 시각. */
  nowSecs?: number;
}) {
  const view = describeConnection(state, nowSecs ?? Math.floor(Date.now() / 1000));
  return (
    <div className="flex items-center gap-3 border-b border-border bg-surface px-4 py-2">
      <div className="min-w-0 flex-1">
        <StatusPill tone={view.tone}>{view.title}</StatusPill>
        {view.detail ? (
          <div className="mt-0.5 truncate text-xs text-text-muted">{view.detail}</div>
        ) : null}
      </div>
      {view.retryable ? (
        <button
          type="button"
          onClick={onRetry}
          className="min-h-[36px] shrink-0 rounded-md border border-border px-3 text-xs text-text"
        >
          다시 시도
        </button>
      ) : null}
    </div>
  );
}
