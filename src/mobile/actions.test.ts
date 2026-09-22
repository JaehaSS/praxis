import { describe, expect, it } from "vitest";
import type { Task } from "../lib/ipc";
import { APPROVE, availableActions, DISCARD, followupAvailability, isTerminal } from "./actions";

function task(state: string): Task {
  return {
    id: 1,
    host: "local",
    repo: "/repo",
    branch: "b",
    base: "main",
    worktree_path: "/wt",
    instruction: "x",
    state,
    created_at: 0,
    updated_at: 0,
    mode: "terminal",
  };
}

describe("availableActions", () => {
  it("검토 대기에서만 종결 버튼을 준다", () => {
    expect(availableActions(task("AwaitingReview"))).toEqual([APPROVE, DISCARD]);
  });

  it("그 외 상태에서는 아무 버튼도 없다", () => {
    // 진행 중 작업에 승인 버튼이 보이면 오탭 한 번이 사고가 된다.
    for (const state of ["Running", "Queued", "Created", "Finalizing", "Done", "Discarded", "Failed"]) {
      expect(availableActions(task(state))).toEqual([]);
    }
  });
});

describe("확인 문구", () => {
  it("되돌릴 수 있는지 분명히 말한다", () => {
    expect(APPROVE.confirm).toContain("되돌리려면");
    // 폐기는 브랜치를 남긴다 — 문구가 "복구 불가"라고 말하면 거짓이 된다(설계 0056).
    expect(DISCARD.confirm).toContain("브랜치에 커밋되어 남습니다");
    expect(DISCARD.confirm).not.toContain("복구할 수 없습니다");
  });

  it("버리기는 위험 스타일이다", () => {
    expect(DISCARD.variant).toBe("danger");
    expect(APPROVE.variant).toBe("primary");
  });
});

describe("followupAvailability", () => {
  it("검토 대기 대화 작업만 이어갈 수 있다", () => {
    // Runner의 requeue_conversation_followup과 같은 조건이다. 여기서 먼저 막지 않으면
    // 사용자는 긴 지시를 다 쓴 뒤에야 409를 본다.
    expect(followupAvailability({ mode: "conversation", state: "AwaitingReview" })).toEqual({
      canSend: true,
    });
  });

  it("실행 중이면 기다리라고 말한다", () => {
    for (const state of ["Running", "Queued", "Starting", "Finalizing"]) {
      const result = followupAvailability({ mode: "conversation", state });
      expect(result.canSend).toBe(false);
      expect(result.reason).toContain("턴이 끝나면");
    }
  });

  it("종료된 작업과 터미널 작업은 사유가 다르다", () => {
    expect(followupAvailability({ mode: "conversation", state: "Done" }).reason).toContain(
      "새 작업",
    );
    expect(followupAvailability({ mode: "terminal", state: "AwaitingReview" }).reason).toContain(
      "터미널",
    );
  });
});

describe("isTerminal", () => {
  it("종료 상태를 구분한다", () => {
    expect(isTerminal("Done")).toBe(true);
    expect(isTerminal("Discarded")).toBe(true);
    expect(isTerminal("Failed")).toBe(true);
    expect(isTerminal("Running")).toBe(false);
    expect(isTerminal("AwaitingReview")).toBe(false);
  });
});
