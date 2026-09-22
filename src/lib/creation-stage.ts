// 세션 생성 파이프라인의 진행 단계 → 컴포저에 보일 한 줄.
// 백엔드 이벤트 `task://creating`의 거울이다 (설계 0059 §5.1).

export type CreationStage = "refresh" | "worktree" | "bootstrap" | "memory" | "spawn";

export interface CreatingEvent {
  client_ref: string;
  stage: CreationStage;
}

/**
 * 진행 중인 생성 하나. `ref`가 null이면 백엔드 이벤트를 받지 않는 생성이며
 * (앙상블·today_start) 단계 대신 후보 수만 보인다.
 */
export interface CreationState {
  ref: string | null;
  stage: CreationStage | null;
  candidates?: number;
}

/** 단계 문구. 아직 이벤트가 오지 않은 동안(null)에도 잠겼다는 것은 보여야 한다. */
export function stageLabel(stage: CreationStage | null, baseBranch?: string): string {
  switch (stage) {
    case "refresh":
      return `base 최신화(origin/${baseBranch || "base"})`;
    case "worktree":
      return "워크트리 생성";
    case "bootstrap":
      return "환경 부트스트랩";
    case "memory":
      return "메모리 투영";
    case "spawn":
      return "에이전트 기동";
    case null:
      return "세션 준비 중";
  }
}

/** 앙상블 문구 — 후보마다 단계가 따로 흐르므로 개수만 알린다. */
export function ensembleLabel(candidates: number): string {
  return `세션 준비 중 · 후보 ${candidates}개`;
}

/** 경과 시간 한 조각. 낭독되지 않는 보조 정보라 소수 한 자리로 충분하다. */
export function elapsedLabel(ms: number): string {
  return `${Math.max(0, ms / 1000).toFixed(1)}s`;
}
