// base 브랜치 최신화 결과 → 사용자에게 보일 한 줄.
// 백엔드 `worktree::refresh::RefreshOutcome`의 거울이다.

export type RefreshOutcome =
  | { kind: "skipped" }
  | { kind: "no_upstream" }
  | { kind: "already_current" }
  | { kind: "fast_forwarded"; commits: number }
  | { kind: "branched_from_remote" }
  | { kind: "diverged"; ahead: number; behind: number }
  | { kind: "failed"; reason: string };

export interface BaseRefreshEvent {
  id: number;
  outcome: RefreshOutcome;
}

/**
 * 보일 문구. **정상 두 가지는 `null`을 준다** — 매번 알리면 그 줄은 곧 안 읽힌다.
 *
 * 어느 문구도 상태색을 쓰지 않는다. 최신화가 안 됐어도 **작업은 성공했고**,
 * 상태색은 상태 표시 전용이다(DESIGN.md).
 */
export function refreshMessage(base: string, o: RefreshOutcome): string | null {
  switch (o.kind) {
    case "skipped":
    case "already_current":
      return null;
    case "fast_forwarded":
      return `${base}을(를) 원격 최신으로 맞췄습니다 (+${o.commits})`;
    case "branched_from_remote":
      return `${base}이(가) 다른 워크트리에서 사용 중이라 origin/${base}에서 분기했습니다.`;
    case "no_upstream":
      return `${base}은(는) 원격에 없습니다 — 로컬 상태에서 분기합니다.`;
    case "diverged":
      return `${base}이(가) 원격과 갈라져 있어 최신화하지 않았습니다 (앞 ${o.ahead} · 뒤 ${o.behind}). 로컬 상태에서 분기합니다.`;
    case "failed":
      return `최신화 실패: ${o.reason} — 로컬 상태에서 분기합니다.`;
  }
}
