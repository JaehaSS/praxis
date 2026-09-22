import type { CrystallizeResult, InterviewAnswer, InterviewAssessment } from "./ipc";

/** 인터뷰 진행 단계 — idle → assessing → (answering) → crystallizing → done, 실패 시 error. */
export type InterviewPhase =
  | "idle"
  | "assessing"
  | "answering"
  | "crystallizing"
  | "done"
  | "error";

export interface InterviewState {
  phase: InterviewPhase;
  /** 인터뷰를 시작한 시점의 지시문 — 신선도(isStale) 판정 기준. */
  instructionSnapshot: string;
  /** 인터뷰를 시작한 시점의 레포 — 같은 지시문이라도 레포가 바뀌면 결과는 무효. */
  repoSnapshot: string;
  /** 최신 요청 세대 — 늦게 도착한 이전 요청의 응답(assessed/crystallized/failed)을 무시한다. */
  requestId: number;
  assessment: InterviewAssessment | null;
  /** 질문 id → 답변. 스킵된 질문은 키 없음. */
  answers: Record<string, string>;
  result: CrystallizeResult | null;
  error: string | null;
}

export type InterviewAction =
  | { type: "start"; instruction: string; repo: string; requestId: number }
  | { type: "assessed"; assessment: InterviewAssessment; requestId: number }
  | { type: "answer"; questionId: string; answer: string }
  | { type: "crystallize"; requestId: number }
  | { type: "crystallized"; result: CrystallizeResult; requestId: number }
  | { type: "failed"; error: string; requestId: number }
  | { type: "retry" }
  | { type: "reset" };

export function initialInterviewState(): InterviewState {
  return {
    phase: "idle",
    instructionSnapshot: "",
    repoSnapshot: "",
    requestId: 0,
    assessment: null,
    answers: {},
    result: null,
    error: null,
  };
}

/** 순수 전이 함수 — 네트워크 호출은 오케스트레이터가 수행하고 결과만 액션으로 전달한다. */
export function interviewReducer(state: InterviewState, action: InterviewAction): InterviewState {
  switch (action.type) {
    case "start":
      return {
        ...initialInterviewState(),
        phase: "assessing",
        instructionSnapshot: action.instruction,
        repoSnapshot: action.repo,
        requestId: action.requestId,
      };
    case "assessed":
      if (action.requestId !== state.requestId) return state; // 이전 세대의 늦은 응답 무시
      // 질문 0개(명확 판정) → answering을 건너뛰고 바로 결정화 대기 상태로.
      // 오케스트레이터는 이 판정을 보고 즉시 결정화를 연쇄 호출한다(Plan 0021 DR-P2).
      return {
        ...state,
        phase: action.assessment.questions.length === 0 ? "crystallizing" : "answering",
        assessment: action.assessment,
        error: null,
      };
    case "answer": {
      const answers = { ...state.answers };
      // 빈 답변 = 스킵 (키 제거) — 결정화 요청에 포함하지 않는다.
      if (action.answer.trim()) answers[action.questionId] = action.answer;
      else delete answers[action.questionId];
      return { ...state, answers };
    }
    case "crystallize":
      // 수동 결정화 버튼은 answering에서만 그려진다 — 그 밖에서 오는 crystallize는
      // reset으로 닫은 세대를 되살릴 뿐이라 정당한 것이 없다(설계 0062 D-1b).
      if (state.phase !== "answering") return state;
      return { ...state, phase: "crystallizing", requestId: action.requestId, error: null };
    case "crystallized":
      if (action.requestId !== state.requestId) return state;
      return { ...state, phase: "done", result: action.result, error: null };
    case "failed":
      if (action.requestId !== state.requestId) return state;
      return { ...state, phase: "error", error: action.error };
    case "retry":
      // 재시도 granularity: 1차(채점) 실패면 처음부터, 2차(결정화) 실패면 답변을 보존한 채 answering으로.
      return state.assessment === null
        ? { ...state, phase: "idle", error: null }
        : { ...state, phase: "answering", error: null };
    case "reset":
      return initialInterviewState();
  }
}

/** 모호성 배지 3구간 — ≤0.2 초록 / ≤0.5 노랑 / >0.5 빨강 (소프트 게이트, 차단 없음). */
export function badgeTier(score: number): "green" | "yellow" | "red" {
  if (score <= 0.2) return "green";
  if (score <= 0.5) return "yellow";
  return "red";
}

/** 인터뷰 시작 후 지시문 또는 레포가 바뀌면 결과(점수·드래프트)가 현재 입력을 대표하지 않는다. */
export function isStale(
  state: InterviewState,
  currentInstruction: string,
  currentRepo: string,
): boolean {
  if (state.phase === "idle") return false;
  return (
    state.instructionSnapshot.trim() !== currentInstruction.trim() ||
    state.repoSnapshot.trim() !== currentRepo.trim()
  );
}

// ---- 오케스트레이션 (App에서 분리 — 자동 연쇄·신선도 게이트를 단위 테스트 가능하게) ----

/** 인터뷰 IPC 의존성 — 실제 구현은 transport, 테스트는 스텁 주입. */
export interface InterviewApi {
  start(repo: string, instruction: string, agent: string): Promise<InterviewAssessment>;
  crystallize(
    repo: string,
    instruction: string,
    answers: InterviewAnswer[],
    agent: string,
  ): Promise<CrystallizeResult>;
}

export interface InterviewFlowDeps {
  api: InterviewApi;
  dispatch: (action: InterviewAction) => void;
}

interface FlowParams {
  repo: string;
  instruction: string;
  agent: string;
  requestId: number;
}

/** 2차(결정화) 실행 — 모호성 점수를 확정한다. 응답이 늦게 도착해 지시문·레포가 이미 바뀌었으면
 *  requestId 세대 검사와 isStale이 걸러내므로, 여기서는 결과만 상태로 넘긴다(소프트 게이트). */
export async function runCrystallizeFlow(
  deps: InterviewFlowDeps,
  params: FlowParams,
  answers: InterviewAnswer[],
): Promise<void> {
  deps.dispatch({ type: "crystallize", requestId: params.requestId });
  await requestCrystallize(deps, params, answers);
}

/** 결정화 API 호출만 — phase 전이(crystallize 디스패치)는 하지 않는다. 연쇄 경로는 이미
 *  assessed가 crystallizing으로 옮겼고, 다시 전이하면 그 사이의 reset이 무효화된다(D-1b). */
async function requestCrystallize(
  deps: InterviewFlowDeps,
  params: FlowParams,
  answers: InterviewAnswer[],
): Promise<void> {
  try {
    const result = await deps.api.crystallize(
      params.repo,
      params.instruction,
      answers,
      params.agent,
    );
    deps.dispatch({ type: "crystallized", result, requestId: params.requestId });
  } catch (error) {
    // 실패해도 컴포저는 잠기지 않는다 — 수동 편집 폴백.
    deps.dispatch({ type: "failed", error: String(error), requestId: params.requestId });
  }
}

/** 1차(채점+질문) 실행. 질문 0개(명확 판정)면 즉시 결정화를 연쇄 호출한다(DR-P2). */
export async function runAssessmentFlow(
  deps: InterviewFlowDeps,
  params: FlowParams,
): Promise<void> {
  deps.dispatch({
    type: "start",
    instruction: params.instruction,
    repo: params.repo,
    requestId: params.requestId,
  });
  try {
    const assessment = await deps.api.start(params.repo, params.instruction, params.agent);
    deps.dispatch({ type: "assessed", assessment, requestId: params.requestId });
    if (assessment.questions.length === 0) {
      await requestCrystallize(deps, params, []);
    }
  } catch (error) {
    deps.dispatch({ type: "failed", error: String(error), requestId: params.requestId });
  }
}
