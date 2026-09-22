import type { QuizAvailability, TaskActivityRow } from "./ipc";

/**
 * 대기가 이만큼 지나야 퀴즈가 뜬다.
 *
 * 백엔드 `quiz::gate::DEFAULT_THRESHOLD_SECS`와 **같은 값이어야 한다**. 단위는 초다 —
 * `started_at`이 초로 내려오므로 밀리초로 두면 임계값이 1000배가 되어 영영 안 열린다.
 */
export const QUIZ_THRESHOLD_SECS = 25;

/**
 * 긴 대기인지 **예측하지 않고 관찰한다** (설계 0044 DR-2).
 *
 * 대기 시작 시점에는 길이를 알 수 없다. 이미 임계값을 넘긴 대기만 연다 — 에이전트 작업
 * 시간이 heavy-tail이라 경과 시간 자체가 가장 강한 신호다.
 */
export function shouldOffer(
  nowSecs: number,
  startedAtSecs: number,
  thresholdSecs = QUIZ_THRESHOLD_SECS,
): boolean {
  // 시계가 뒤로 가면 경과가 음수다 — 그때 열리면 짧은 대기에도 퀴즈가 뜬다.
  return nowSecs - startedAtSecs >= thresholdSecs;
}

/**
 * 지금 퀴즈를 띄울 만한 대기가 하나라도 있는가.
 *
 * 가장 오래 기다린 작업을 기준으로 본다 — 여러 작업이 돌 때 하나라도 길어졌으면 그 사람은
 * 이미 기다리고 있는 것이다.
 */
export function gateIsOpen(
  rows: readonly TaskActivityRow[],
  nowSecs: number,
  thresholdSecs = QUIZ_THRESHOLD_SECS,
): boolean {
  return rows.some((row) => shouldOffer(nowSecs, row.started_at, thresholdSecs));
}

/** 밀리초 시각을 초로. `Date.now()`를 백엔드 단위에 맞출 때 쓴다. */
export function toSeconds(millis: number): number {
  return Math.floor(millis / 1000);
}

/** 대기 중에 띄울 화면. */
export type QuizMode = "quiz" | "review" | "insight";

/**
 * 큐 상태를 보고 무엇을 띄울지 — 띄우지 않을지 — 정한다 (이슈 #87).
 *
 * 낼 문제가 없는데 여는 것이 문제였다. 그때 화면이 할 수 있는 말은 "없습니다"뿐이고, 그게
 * 요구하는 행동(스케줄 등록)은 대기 중에 할 일이 아니다.
 *
 * 다만 **출제 0 + 검수 N**은 자주 나오는 정상 상태다 — 도메인 문제는 승인 전엔 출제되지
 * 않으므로, 검수야말로 지금 할 수 있는 일이다.
 */
export function modeFor(counts: QuizAvailability, insightCards = 0): QuizMode | null {
  if (counts.askable > 0) return "quiz";
  if (counts.pending_review > 0) return "review";
  // **퀴즈가 인사이트를 이긴다.** 능동 회상이 수동 읽기보다 학습 이득이 크고, 복습 카드는
  // 만기가 있어 미루면 손해가 누적된다 — 인사이트는 언제 봐도 같다.
  if (insightCards > 0) return "insight";
  return null;
}
