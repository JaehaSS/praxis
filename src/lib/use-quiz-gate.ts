import { useCallback, useEffect, useState } from "react";

import { insightAvailability, quizAvailability, taskActivity } from "./ipc";
import { QUIZ_THRESHOLD_SECS, gateIsOpen, modeFor, toSeconds, type QuizMode } from "./quiz";

/**
 * 게이트 확인 주기.
 *
 * 활동 폴링보다 성기다 — 여기서 재는 것은 "지금 무슨 도구를 쓰는가"가 아니라
 * "충분히 오래 기다렸는가"라서 초 단위 정밀도가 필요 없다.
 */
export const QUIZ_GATE_POLL_MS = 5_000;

export interface QuizGate {
  /** 지금 띄울 화면. 닫혀 있으면 `null`. */
  mode: QuizMode | null;
  /** 검수 대기 건수 — 퀴즈 화면이 검수 통로를 보일지 정하는 데 쓴다. */
  pendingReview: number;
  /** 검수 화면으로 건너간다. */
  showReview: () => void;
  /** 검수 화면에서 나온다 — 퀴즈에서 왔으면 퀴즈로, 검수로 열렸으면 닫는다. */
  leaveReview: () => void;
  /** 검수 화면에서 되돌아갈 퀴즈가 있는가. */
  canReturnToQuiz: boolean;
  /** 사용자가 닫았다. 다음 대기가 시작되면 다시 열릴 수 있다. */
  dismiss: () => void;
}

/**
 * 대기가 임계값을 넘겼고 **띄울 것이 실제로 있는지** 관찰한다 (설계 0044 DR-2, 이슈 #87).
 *
 * 경과만 보고 열면 큐가 비었을 때 "출제할 문제가 없습니다"만 턴마다 반복된다. 그 화면이
 * 요구하는 행동은 대기 중에 할 일이 아니므로, 아예 열지 않는 것이 맞다.
 *
 * `active`는 **이 대화의 턴이 진행 중인가**다. `taskId`를 주면 그 작업의 대기만 잰다 —
 * 카드가 대화 흐름 안에 놓이므로 다른 세션의 긴 대기가 이 대화에 카드를 띄우면 안 된다.
 *
 * 한 번 열리면 **응답이 도착해도 스스로 닫지 않는다**(DR-3) — 닫는 것은 사용자가 정한다.
 * 그래서 `active`가 꺼지는 것은 아무것도 바꾸지 않는다. 지우는 시점은 **다음 대기가 시작될 때**
 * 하나다: 지난 대기의 화면·사용자가 닫은 사실이 그때 함께 지워진다. 안 지우면 한 번 닫은 뒤로
 * 영영 퀴즈를 못 보고, 열린 뒤에도 계속 다시 판정하면 풀던 도중에 화면이 바뀐다.
 */
export function useQuizGate(
  active: boolean,
  taskId: number | null = null,
  thresholdSecs = QUIZ_THRESHOLD_SECS,
): QuizGate {
  const [gateMode, setGateMode] = useState<QuizMode | null>(null);
  /** 사용자가 화면을 직접 바꿨다. 게이트가 정한 것보다 우선한다. */
  const [override, setOverride] = useState<QuizMode | null>(null);
  const [pendingReview, setPendingReview] = useState(0);
  const [dismissed, setDismissed] = useState(false);

  // 새 대기가 시작됐다 — 지난 대기의 흔적을 지운다. 턴이 끝날 때는 아무것도 하지 않는다(DR-3).
  useEffect(() => {
    if (!active) return;
    setGateMode(null);
    setOverride(null);
    setPendingReview(0);
    setDismissed(false);
  }, [active]);

  useEffect(() => {
    if (!active) return;
    if (gateMode) return; // 이미 열렸다 — 무엇을 띄울지는 정해졌다.

    let cancelled = false;
    const check = async () => {
      try {
        const rows = await taskActivity();
        const mine = taskId == null ? rows : rows.filter((row) => row.task_id === taskId);
        if (cancelled || !gateIsOpen(mine, toSeconds(Date.now()), thresholdSecs)) return;
        // 시간 게이트를 넘긴 뒤에야 큐를 본다 — 짧은 대기에는 조회 자체가 낭비다.
        // **병렬로 묻는다.** 순차로 부르면 게이트가 열리는 데 왕복이 두 번 걸린다.
        // 인사이트 조회가 실패해도 퀴즈는 떠야 하므로 각자 폴백을 갖는다.
        const [counts, insight] = await Promise.all([
          quizAvailability(),
          insightAvailability().catch(() => ({ cards: 0, decks: 0, enabled: false, warnings: [] })),
        ]);
        if (cancelled) return;
        setPendingReview(counts.pending_review);
        // 꺼져 있으면 카드가 있어도 띄우지 않는다 — 판단은 여기서 한다.
        const next = modeFor(counts, insight.enabled ? insight.cards : 0);
        if (next) setGateMode(next);
      } catch {
        // 조회 실패는 게이트를 열지 않는다 — 퀴즈가 안 뜨는 쪽이 안전하다.
      }
    };
    void check();
    const timer = setInterval(() => void check(), QUIZ_GATE_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [active, taskId, thresholdSecs, gateMode]);

  const leaveReview = useCallback(() => {
    // 퀴즈를 보다 건너온 것이면 되돌아간다. 검수만 있어서 열린 것이면 돌아갈 곳이 없다.
    if (override === "review") setOverride(null);
    else setDismissed(true);
  }, [override]);

  // `showReview`/`dismiss`는 자식의 `useCallback` deps에 들어간다 — 매 렌더 새로 만들면
  // 자식의 로더가 재생성돼 effect가 무한히 다시 돈다.
  const showReview = useCallback(() => setOverride("review"), []);
  const dismiss = useCallback(() => setDismissed(true), []);

  return {
    mode: dismissed ? null : (override ?? gateMode),
    pendingReview,
    showReview,
    leaveReview,
    canReturnToQuiz: override === "review",
    dismiss,
  };
}
