import type { Task } from "../lib/ipc";

// 작업에 대해 지금 취할 수 있는 행동 판정 — 순수 로직. (설계 0013 §10 M2)
//
// 판정을 UI에서 흩뿌리지 않고 한곳에 모으는 이유: 폰에서는 오탭 한 번이 머지가 된다.
// "언제 버튼이 보이는가"가 곧 안전장치이므로 테스트로 고정한다.

export type ActionKind = "approve" | "discard";

export interface ActionSpec {
  kind: ActionKind;
  label: string;
  /** 확인 시트에 띄울 문구 — 무엇이 일어나는지 되돌릴 수 있는지 분명히 말한다. */
  confirm: string;
  variant: "primary" | "danger";
}

export const APPROVE: ActionSpec = {
  kind: "approve",
  label: "승인하고 머지",
  confirm: "변경을 base 브랜치에 머지합니다. 되돌리려면 git에서 직접 되돌려야 합니다.",
  variant: "primary",
};

export const DISCARD: ActionSpec = {
  kind: "discard",
  label: "버리기",
  confirm: "worktree를 걷어냅니다. 버리기 직전 상태는 브랜치에 커밋되어 남습니다.",
  variant: "danger",
};

/**
 * 검토 대기 상태에서만 종결할 수 있다. 이것은 Runner의 CAS 계약과 같은 조건이며,
 * UI에서 먼저 막는 것은 편의가 아니라 오탭 방지다(서버가 최종 판단은 그대로 한다).
 */
export function availableActions(task: Task): ActionSpec[] {
  return task.state === "AwaitingReview" ? [APPROVE, DISCARD] : [];
}

/** 종결 후 화면이 무엇을 보여줘야 하는지 — 상태는 서버가 정하므로 재조회 신호만 낸다. */
export function isTerminal(state: string): boolean {
  return state === "Done" || state === "Discarded" || state === "Failed";
}

/**
 * 지금 후속 메시지를 보낼 수 있는지. Runner는 **검토 대기 상태의 conversation 작업**만
 * 이어받는다(`requeue_conversation_followup`). UI에서 먼저 판정하지 않으면 사용자는
 * 입력을 다 쓴 뒤에야 409를 보게 된다.
 *
 * 막는 이유를 문장으로 함께 돌려준다 — 버튼만 비활성이면 왜 안 되는지 알 수 없다.
 */
export function followupAvailability(task: {
  mode: string;
  state: string;
}): { canSend: boolean; reason?: string } {
  if (task.mode !== "conversation") {
    return { canSend: false, reason: "터미널 작업에는 후속 메시지를 보낼 수 없습니다." };
  }
  if (task.state === "AwaitingReview") return { canSend: true };
  if (isTerminal(task.state)) {
    return { canSend: false, reason: "종료된 작업입니다. 새 작업으로 이어가세요." };
  }
  return { canSend: false, reason: "에이전트가 실행 중입니다. 턴이 끝나면 보낼 수 있습니다." };
}
