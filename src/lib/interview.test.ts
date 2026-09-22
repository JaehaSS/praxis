import { describe, expect, it, vi } from "vitest";

import type { CrystallizeResult, InterviewAssessment } from "./ipc";
import {
  badgeTier,
  initialInterviewState,
  interviewReducer,
  isStale,
  runAssessmentFlow,
  runCrystallizeFlow,
  type InterviewAction,
  type InterviewFlowDeps,
  type InterviewState,
} from "./interview";

const score = (value: number) => ({ score: value, goal: 0.5, constraints: 0.5, success: 0.5 });

const assessmentWith = (questionCount: number): InterviewAssessment => ({
  ambiguity: score(0.4),
  questions: Array.from({ length: questionCount }, (_, index) => ({
    id: `q${index + 1}`,
    dimension: "goal",
    text: `질문 ${index + 1}`,
    reason: "이유",
    options: ["보기 1", "보기 2"],
  })),
});

const crystallized: CrystallizeResult = {
  ambiguity: score(0.1),
  acceptance: ["cargo test 통과"],
  stop_conditions: [],
  must_preserve: [],
  protected_paths: [],
  non_goals: [],
  dropped: 0,
};

const start = (instruction = "지시문", repo = "/repo", requestId = 1): InterviewAction => ({
  type: "start",
  instruction,
  repo,
  requestId,
});

const run = (state: InterviewState, ...actions: InterviewAction[]) =>
  actions.reduce(interviewReducer, state);

describe("interviewReducer", () => {
  it("전이 표: idle → assessing → answering → crystallizing → done", () => {
    let s = interviewReducer(initialInterviewState(), start());
    expect(s.phase).toBe("assessing");
    expect(s.instructionSnapshot).toBe("지시문");
    expect(s.repoSnapshot).toBe("/repo");

    s = interviewReducer(s, { type: "assessed", assessment: assessmentWith(2), requestId: 1 });
    expect(s.phase).toBe("answering");

    s = interviewReducer(s, { type: "answer", questionId: "q1", answer: "답변" });
    expect(s.answers).toEqual({ q1: "답변" });

    s = interviewReducer(s, { type: "crystallize", requestId: 2 });
    expect(s.phase).toBe("crystallizing");

    s = interviewReducer(s, { type: "crystallized", result: crystallized, requestId: 2 });
    expect(s.phase).toBe("done");
    expect(s.result).toBe(crystallized);
  });

  it("질문 0개면 answering을 건너뛰고 crystallizing으로 간다", () => {
    const s = run(initialInterviewState(), start(), {
      type: "assessed",
      assessment: assessmentWith(0),
      requestId: 1,
    });
    expect(s.phase).toBe("crystallizing");
  });

  it("이전 세대(requestId 불일치)의 늦은 응답은 무시한다", () => {
    // 요청 1이 느린 사이 요청 2를 새로 시작 → 요청 1의 늦은 assessed/failed는 무시돼야 한다
    let s = run(initialInterviewState(), start("첫 지시문", "/repo", 1), start("새 지시문", "/repo", 2));
    const late = interviewReducer(s, {
      type: "assessed",
      assessment: assessmentWith(3),
      requestId: 1,
    });
    expect(late).toBe(s);
    expect(late.assessment).toBeNull();

    s = interviewReducer(s, { type: "assessed", assessment: assessmentWith(0), requestId: 2 });
    const lateFail = interviewReducer(s, { type: "failed", error: "옛 요청 실패", requestId: 1 });
    expect(lateFail.phase).toBe("crystallizing");
    const lateResult = interviewReducer(s, {
      type: "crystallized",
      result: crystallized,
      requestId: 1,
    });
    expect(lateResult.result).toBeNull();
  });

  it("빈 답변은 스킵으로 취급해 키를 제거한다", () => {
    let s = run(
      initialInterviewState(),
      start(),
      { type: "assessed", assessment: assessmentWith(1), requestId: 1 },
      { type: "answer", questionId: "q1", answer: "임시" },
      { type: "answer", questionId: "q1", answer: "  " },
    );
    expect(s.answers).toEqual({});
    s = interviewReducer(s, { type: "answer", questionId: "q2", answer: "" });
    expect(Object.keys(s.answers)).toHaveLength(0);
  });

  it("실패 → error → retry: 채점 실패는 idle로, 결정화 실패는 답변 보존한 answering으로", () => {
    let s = run(initialInterviewState(), start(), { type: "failed", error: "타임아웃", requestId: 1 });
    expect(s.phase).toBe("error");
    expect(s.error).toBe("타임아웃");
    expect(interviewReducer(s, { type: "retry" }).phase).toBe("idle");

    s = run(
      initialInterviewState(),
      start(),
      { type: "assessed", assessment: assessmentWith(1), requestId: 1 },
      { type: "answer", questionId: "q1", answer: "답" },
      { type: "crystallize", requestId: 2 },
      { type: "failed", error: "파싱 실패", requestId: 2 },
    );
    const retried = interviewReducer(s, { type: "retry" });
    expect(retried.phase).toBe("answering");
    expect(retried.answers).toEqual({ q1: "답" });
  });

  it("reset은 초기 상태로 되돌린다", () => {
    const s = run(
      initialInterviewState(),
      start(),
      { type: "assessed", assessment: assessmentWith(0), requestId: 1 },
      { type: "crystallized", result: crystallized, requestId: 1 },
      { type: "reset" },
    );
    expect(s).toEqual(initialInterviewState());
  });

  it("reset 뒤 도착한 이전 세대 응답은 상태를 되살리지 못한다", () => {
    // ✕로 닫은 뒤 늦게 온 assessed/crystallized — 세대 가드가 버려 result는 null로 남는다(A-1.3).
    const closed = run(initialInterviewState(), start(), { type: "reset" });
    const late = run(
      closed,
      { type: "assessed", assessment: assessmentWith(0), requestId: 1 },
      { type: "crystallized", result: crystallized, requestId: 1 },
    );
    expect(late.phase).toBe("idle");
    expect(late.result).toBeNull();
  });

  it("reset 뒤 다시 시작하면 새 세대가 반영된다", () => {
    const s = run(
      initialInterviewState(),
      start(),
      { type: "reset" },
      start("새 지시문", "/repo", 2),
      { type: "assessed", assessment: assessmentWith(2), requestId: 2 },
    );
    expect(s.phase).toBe("answering");
    expect(s.instructionSnapshot).toBe("새 지시문");
  });

  it("answering 밖에서 온 crystallize는 무시한다", () => {
    // 수동 결정화 버튼은 answering에서만 그려진다 — 그 밖의 crystallize는 닫힌 세대를 되살릴 뿐이다(D-1b).
    const idle = initialInterviewState();
    expect(interviewReducer(idle, { type: "crystallize", requestId: 3 })).toBe(idle);
    const assessing = interviewReducer(idle, start());
    expect(interviewReducer(assessing, { type: "crystallize", requestId: 3 })).toBe(assessing);
  });
});

describe("badgeTier", () => {
  it("경계값: 0.2까지 green, 0.5까지 yellow, 초과는 red", () => {
    expect(badgeTier(0)).toBe("green");
    expect(badgeTier(0.2)).toBe("green");
    expect(badgeTier(0.21)).toBe("yellow");
    expect(badgeTier(0.5)).toBe("yellow");
    expect(badgeTier(0.51)).toBe("red");
    expect(badgeTier(1)).toBe("red");
  });
});

describe("isStale", () => {
  it("idle이면 stale 아님, 시작 후 지시문 또는 레포 변경 시 stale", () => {
    const idle = initialInterviewState();
    expect(isStale(idle, "아무거나", "/repo")).toBe(false);

    const started = interviewReducer(idle, start("원본 지시문", "/repo-a"));
    expect(isStale(started, "원본 지시문", "/repo-a")).toBe(false);
    expect(isStale(started, "  원본 지시문  ", "/repo-a")).toBe(false);
    expect(isStale(started, "바뀐 지시문", "/repo-a")).toBe(true);
    // 같은 지시문이라도 레포가 바뀌면 stale — 옛 레포 기반 protected_paths가 새 레포에 붙는 것 방지
    expect(isStale(started, "원본 지시문", "/repo-b")).toBe(true);
  });
});

describe("interview flows (오케스트레이션)", () => {
  const makeDeps = (overrides: Partial<InterviewFlowDeps> = {}) => {
    const actions: InterviewAction[] = [];
    const deps: InterviewFlowDeps = {
      api: {
        start: vi.fn(async () => assessmentWith(1)),
        crystallize: vi.fn(async () => crystallized),
      },
      dispatch: (action) => actions.push(action),
      ...overrides,
    };
    return { deps, actions };
  };
  const params = { repo: "/repo", instruction: "지시문", agent: "claude", requestId: 1 };

  it("질문 0개(명확 판정)면 결정화를 자동 연쇄 호출한다", async () => {
    const { deps, actions } = makeDeps();
    (deps.api.start as ReturnType<typeof vi.fn>).mockResolvedValue(assessmentWith(0));
    await runAssessmentFlow(deps, params);
    expect(deps.api.crystallize).toHaveBeenCalledWith("/repo", "지시문", [], "claude");
    // 연쇄는 crystallize를 디스패치하지 않는다 — assessed가 이미 crystallizing으로 옮겼고,
    // 다시 디스패치하면 그 사이의 reset이 무효화된다(D-1b).
    expect(actions.map((a) => a.type)).toEqual(["start", "assessed", "crystallized"]);
  });

  it("연쇄 결정화 도중 reset하면 닫힌 채로 남는다", async () => {
    // ✕는 api.start 응답 전에 눌린다 — 늦게 온 assessed·crystallized는 세대 가드가 버려야 한다(A-1.2).
    let state = initialInterviewState();
    const actions: InterviewAction[] = [];
    let releaseStart = () => {};
    const started = new Promise<void>((resolve) => {
      releaseStart = resolve;
    });
    const deps: InterviewFlowDeps = {
      api: {
        start: async () => {
          await started;
          return assessmentWith(0);
        },
        crystallize: async () => crystallized,
      },
      dispatch: (action) => {
        actions.push(action);
        state = interviewReducer(state, action);
      },
    };

    const flow = runAssessmentFlow(deps, params);
    deps.dispatch({ type: "reset" });
    releaseStart();
    await flow;

    expect(state.phase).toBe("idle");
    expect(state.result).toBeNull();
    expect(actions.map((a) => a.type)).not.toContain("crystallize");
  });

  it("질문이 있으면 자동 연쇄하지 않는다", async () => {
    const { deps } = makeDeps();
    await runAssessmentFlow(deps, params);
    expect(deps.api.crystallize).not.toHaveBeenCalled();
  });

  it("결정화 결과는 항상 상태로 전이된다 — 신선도 판정은 requestId·isStale이 맡는다", async () => {
    // 결정화 진행 중 지시문이 바뀌어도 done으로 전이하고, 패널이 stale 경고와 함께 보여준다.
    const { deps, actions } = makeDeps();
    await runCrystallizeFlow(deps, params, []);
    expect(actions.map((a) => a.type)).toEqual(["crystallize", "crystallized"]);
  });

  it("1차 실패는 failed 액션으로 끝나고 결정화를 호출하지 않는다", async () => {
    const { deps, actions } = makeDeps();
    (deps.api.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("CLI 부재"));
    await runAssessmentFlow(deps, params);
    expect(actions.map((a) => a.type)).toEqual(["start", "failed"]);
    expect(deps.api.crystallize).not.toHaveBeenCalled();
  });
});
