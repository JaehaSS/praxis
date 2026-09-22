import type { GrillNote, GrillQuestion, GrillRound, GrillTurn } from "./ipc";

/** 발산 인터뷰 진행 단계 — idle → asking → answering → (반복) → noting → done, 실패 시 error. */
export type GrillPhase = "idle" | "asking" | "answering" | "noting" | "done" | "error";

/** 실패한 호출은 라운드를 소비시키지 않되, 무한 재시도로 호출이 새지도 않게 한다(Plan 0039 DR-P5).
 *  백엔드 상한은 transcript 길이로만 판단하므로 재시도는 프론트가 세야 한다. */
export const MAX_RETRIES_PER_ROUND = 2;

/** `interview::grill::MAX_GRILL_ROUNDS` 미러 — 표시 전용이다.
 *  실제 종료는 백엔드가 강제하므로 이 값이 어긋나도 상한이 무너지지는 않는다. */
export const MAX_GRILL_ROUNDS = 11;

export interface GrillState {
  phase: GrillPhase;
  /** 인터뷰를 시작한 시점의 지시문 — 신선도(isStale) 판정 기준. */
  instructionSnapshot: string;
  /** 같은 지시문이라도 레포가 바뀌면 결과는 무효. */
  repoSnapshot: string;
  /** 최신 요청 세대 — 늦게 도착한 이전 요청의 응답을 무시한다. */
  requestId: number;
  transcript: GrillTurn[];
  current: GrillQuestion | null;
  /** 작성 중인 답변. */
  draft: string;
  openThreads: string[];
  /** 상한 도달로 끊겼는지 — 노트 화면 문구를 자연 종료와 달리한다. */
  forcedEnd: boolean;
  /** 현재 라운드의 재시도 횟수(DR-P5). */
  retries: number;
  note: GrillNote | null;
  savedPath: string | null;
  error: string | null;
}

export type GrillAction =
  | { type: "start"; instruction: string; repo: string; requestId: number }
  | { type: "asked"; round: GrillRound; requestId: number }
  | { type: "draft"; value: string }
  | { type: "answer" }
  | { type: "acceptRecommendation" }
  | { type: "dontKnow" }
  | { type: "endNow" }
  | { type: "noted"; note: GrillNote; requestId: number }
  | { type: "saved"; path: string }
  | { type: "saveFailed"; error: string }
  | { type: "failed"; error: string; requestId: number }
  | { type: "retry" }
  | { type: "reset" };

export function initialGrillState(): GrillState {
  return {
    phase: "idle",
    instructionSnapshot: "",
    repoSnapshot: "",
    requestId: 0,
    transcript: [],
    current: null,
    draft: "",
    openThreads: [],
    forcedEnd: false,
    retries: 0,
    note: null,
    savedPath: null,
    error: null,
  };
}

/** 현재 질문과 답변을 transcript에 밀어 넣고 다음 라운드를 기다리는 상태로 옮긴다. */
function commitAnswer(state: GrillState, answer: string): GrillState {
  if (!state.current) return state;
  return {
    ...state,
    phase: "asking",
    transcript: [
      ...state.transcript,
      { question: state.current.text, recommendation: state.current.recommendation, answer },
    ],
    current: null,
    draft: "",
  };
}

/** 순수 전이 함수 — 네트워크 호출은 flow 함수가 수행하고 결과만 액션으로 전달한다. */
export function grillReducer(state: GrillState, action: GrillAction): GrillState {
  switch (action.type) {
    case "start":
      return {
        ...initialGrillState(),
        phase: "asking",
        instructionSnapshot: action.instruction,
        repoSnapshot: action.repo,
        requestId: action.requestId,
      };
    case "asked": {
      if (action.requestId !== state.requestId) return state; // 이전 세대의 늦은 응답 무시
      const { question, open_threads, forced_end } = action.round;
      return {
        ...state,
        // 질문이 없으면 프론티어 소진 또는 상한 도달 — 노트 단계로 넘어간다.
        phase: question ? "answering" : "noting",
        current: question,
        openThreads: open_threads,
        forcedEnd: forced_end,
        retries: 0, // 성공했으므로 이 라운드의 재시도 카운터를 리셋
        error: null,
      };
    }
    case "draft":
      return { ...state, draft: action.value };
    case "answer":
      return state.draft.trim() ? commitAnswer(state, state.draft.trim()) : state;
    case "acceptRecommendation":
      return state.current ? commitAnswer(state, state.current.recommendation) : state;
    // "모르겠다"는 유효한 답 — 추측을 강요하지 않고 그대로 기록해 모델이 프로토타입 대상으로 남기게 한다.
    case "dontKnow":
      return commitAnswer(state, "모르겠다");
    case "endNow":
      return { ...state, phase: "noting", current: null };
    case "noted":
      if (action.requestId !== state.requestId) return state;
      return { ...state, phase: "done", note: action.note, error: null };
    case "saved":
      return { ...state, savedPath: action.path, error: null };
    // 저장 실패는 phase를 바꾸지 않는다 — 파일 쓰기가 실패했다고 인터뷰로 얻은 노트를
    // 화면에서 지우면 사용자가 12라운드의 결과를 잃는다.
    case "saveFailed":
      return { ...state, error: action.error };
    case "failed":
      if (action.requestId !== state.requestId) return state;
      return { ...state, phase: "error", error: action.error };
    case "retry": {
      // 재시도가 상한에 닿으면 error에 머문다 — 실패한 호출로 총 호출 상한을 넘기지 않는다.
      if (state.retries >= MAX_RETRIES_PER_ROUND) return state;
      // 노트 단계에서 실패했으면 노트를 다시 타고, 그 전이면 질문을 다시 받는다.
      const resumed: GrillPhase =
        state.note === null && state.transcript.length > 0 && state.current === null
          ? "asking"
          : state.current !== null
            ? "answering"
            : "idle";
      return { ...state, phase: resumed, retries: state.retries + 1, error: null };
    }
    case "reset":
      return initialGrillState();
  }
}

/** 인터뷰 시작 후 지시문 또는 레포가 바뀌면 결과가 현재 입력을 대표하지 않는다. */
export function isStale(state: GrillState, instruction: string, repo: string): boolean {
  if (state.phase === "idle") return false;
  return (
    state.instructionSnapshot.trim() !== instruction.trim() ||
    state.repoSnapshot.trim() !== repo.trim()
  );
}

// ---- 오케스트레이션 (interview.ts와 같은 구조 — App은 의존성만 연결한다) ----

export interface GrillApi {
  round(
    repo: string,
    instruction: string,
    transcript: GrillTurn[],
    agent: string,
  ): Promise<GrillRound>;
  note(
    repo: string,
    instruction: string,
    transcript: GrillTurn[],
    agent: string,
  ): Promise<GrillNote>;
  save(repo: string, slug: string, markdown: string, date: string): Promise<string>;
}

export interface GrillFlowDeps {
  api: GrillApi;
  dispatch: (action: GrillAction) => void;
}

interface FlowParams {
  repo: string;
  instruction: string;
  agent: string;
  requestId: number;
}

export async function runGrillRound(
  deps: GrillFlowDeps,
  params: FlowParams,
  transcript: GrillTurn[],
): Promise<void> {
  try {
    const round = await deps.api.round(params.repo, params.instruction, transcript, params.agent);
    deps.dispatch({ type: "asked", round, requestId: params.requestId });
  } catch (error) {
    // 실패해도 컴포저는 잠기지 않는다 — 재시도하거나 수동으로 계속할 수 있다.
    deps.dispatch({ type: "failed", error: String(error), requestId: params.requestId });
  }
}

/** 노트는 최대 120초 걸린다. 그 사이 지시문이 바뀌어도 노트를 버리지 않는다 —
 *  컴포저에 자동 반영하는 경로가 없기 때문이다. 지시문 적용은 사용자가 버튼으로 하고,
 *  입력이 바뀐 사실은 패널의 `isStale` 경고가 알린다. */
export async function runGrillNote(
  deps: GrillFlowDeps,
  params: FlowParams,
  transcript: GrillTurn[],
): Promise<void> {
  try {
    const note = await deps.api.note(params.repo, params.instruction, transcript, params.agent);
    deps.dispatch({ type: "noted", note, requestId: params.requestId });
  } catch (error) {
    deps.dispatch({ type: "failed", error: String(error), requestId: params.requestId });
  }
}
