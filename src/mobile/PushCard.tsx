import { useCallback, useEffect, useState } from "react";
import { Button } from "./primitives";
import {
  currentState,
  describePush,
  isIos,
  isStandalone,
  subscribe,
  unsubscribe,
  type PushState,
} from "./push";

// 알림 설정 카드. (설계 0013 §8)
// iOS는 홈 화면 추가 전에는 구독이 불가능하고 권한이 조용히 철회될 수 있다.
// 그래서 상태를 **항상** 보여준다 — 알림이 안 오는데 그 사실을 모르는 것이 최악이다.

export function PushCard() {
  const [state, setState] = useState<PushState | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    void currentState().then(setState);
  }, []);

  useEffect(refresh, [refresh]);

  if (!state) return null;
  const view = describePush(state);
  const needsHomeScreen = isIos() && !isStandalone();

  return (
    <div className="space-y-3 border-b border-border px-4 py-3">
      <div>
        <div className="text-sm text-text">{view.label}</div>
        {view.detail ? (
          <div className="mt-0.5 text-xs text-text-muted">{view.detail}</div>
        ) : null}
      </div>

      {needsHomeScreen ? (
        <div className="rounded-md border border-border bg-raised px-3 py-2 text-xs text-text-secondary">
          공유 버튼 → &ldquo;홈 화면에 추가&rdquo;를 누른 뒤, 홈 화면 아이콘으로 다시 열어야
          알림을 켤 수 있습니다.
        </div>
      ) : null}

      {state.kind === "subscribed" ? (
        <Button
          disabled={busy}
          onClick={() => {
            setBusy(true);
            void unsubscribe()
              .then(setState)
              .finally(() => setBusy(false));
          }}
        >
          {busy ? "처리 중…" : "알림 끄기"}
        </Button>
      ) : state.kind === "idle" || state.kind === "error" ? (
        <Button
          variant="primary"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            void subscribe()
              .then(setState)
              .finally(() => setBusy(false));
          }}
        >
          {busy ? "처리 중…" : "알림 켜기"}
        </Button>
      ) : null}
    </div>
  );
}
