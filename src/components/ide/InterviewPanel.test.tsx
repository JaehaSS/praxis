// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { GrillState } from "../../lib/grill";
import { initialGrillState } from "../../lib/grill";
import type { InterviewState } from "../../lib/interview";
import { initialInterviewState } from "../../lib/interview";
import type { AmbiguityScore } from "../../lib/ipc";
import { AmbiguityBadge, InterviewPanel, interviewStage } from "./InterviewPanel";

const score = (value: number, dims: Partial<AmbiguityScore> = {}): AmbiguityScore => ({
  score: value,
  goal: 0.9,
  constraints: 0.9,
  success: 0.9,
  ...dims,
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

interface PanelOptions {
  grill?: GrillState;
  interview?: InterviewState;
  currentInstruction?: string;
  currentRepo?: string;
  onClose?: () => void;
  onUndo?: () => void;
  handlers?: Partial<Record<HandlerName, () => void>>;
}

type HandlerName =
  | "onGrillStart"
  | "onGrillApplyAndScore"
  | "onGrillApplyInstruction"
  | "onGrillEndNow"
  | "onInterviewStart"
  | "onInterviewCrystallize";

const noop = () => {};

const panel = ({
  grill = initialGrillState(),
  interview = initialInterviewState(),
  currentInstruction = "지시문",
  currentRepo = "/repo",
  onClose,
  onUndo,
  handlers = {},
}: PanelOptions) => (
  <InterviewPanel
    grill={grill}
    interview={interview}
    currentInstruction={currentInstruction}
    currentRepo={currentRepo}
    disabled={false}
    onGrillStart={handlers.onGrillStart ?? noop}
    onGrillDraft={noop}
    onGrillAnswer={noop}
    onGrillAcceptRecommendation={noop}
    onGrillDontKnow={noop}
    onGrillEndNow={handlers.onGrillEndNow ?? noop}
    onGrillApplyAndScore={handlers.onGrillApplyAndScore ?? noop}
    onGrillApplyInstruction={handlers.onGrillApplyInstruction ?? noop}
    onGrillUndoInstruction={onUndo}
    onGrillSave={noop}
    onGrillRetry={noop}
    onInterviewStart={handlers.onInterviewStart ?? noop}
    onInterviewAnswer={noop}
    onInterviewCrystallize={handlers.onInterviewCrystallize ?? noop}
    onInterviewRetry={noop}
    onClose={onClose}
  />
);

const render = (options: PanelOptions) => renderToStaticMarkup(panel(options));

const mount = (options: PanelOptions) => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  act(() => root.render(panel(options)));
  return host;
};

const clickButton = (host: HTMLElement, label: string) => {
  const button = Array.from(host.querySelectorAll("button")).find(
    (el) => el.textContent?.trim() === label,
  );
  if (!button) throw new Error(`버튼 없음: ${label}`);
  act(() => button.click());
};

const grillNote = (overrides: Partial<GrillState["note"] & object> = {}) => ({
  slug: "s",
  markdown: "## 무엇을 하려는가\n본문",
  revised_instruction: "새 지시문",
  unresolved: [],
  dropped: 0,
  ...overrides,
});

const grillAnswering: GrillState = {
  ...initialGrillState(),
  phase: "answering",
  instructionSnapshot: "지시문",
  repoSnapshot: "/repo",
  current: {
    id: "q1",
    text: "이 기능의 첫 사용자는 누구입니까",
    recommendation: "1인 개발자입니다",
    why: "범위가 갈립니다",
  },
  openThreads: ["저장 시점", "재시도 정책"],
};

const grillDone: GrillState = {
  ...grillAnswering,
  phase: "done",
  current: null,
  transcript: [{ question: "q", recommendation: "r", answer: "a" }],
  note: grillNote(),
};

const interviewAnswering: InterviewState = {
  ...initialInterviewState(),
  phase: "answering",
  instructionSnapshot: "지시문",
  repoSnapshot: "/repo",
  assessment: {
    ambiguity: score(0.6, { goal: 0.4 }),
    questions: [
      {
        id: "q1",
        dimension: "goal",
        text: "무엇을 만드나요?",
        reason: "목표가 불명확",
        options: ["A", "B"],
      },
    ],
  },
};

const interviewDone: InterviewState = {
  ...initialInterviewState(),
  phase: "done",
  instructionSnapshot: "지시문",
  repoSnapshot: "/repo",
  result: {
    ambiguity: score(0.1),
    acceptance: [],
    stop_conditions: [],
    must_preserve: [],
    protected_paths: [],
    non_goals: [],
    dropped: 0,
  },
};

describe("AmbiguityBadge", () => {
  it("3구간 배지 색과 불명확 차원을 표시한다", () => {
    const green = renderToStaticMarkup(<AmbiguityBadge ambiguity={score(0.1)} />);
    expect(green).toContain("bg-status-done/15");
    expect(green).toContain("모호성 0.10");

    const yellow = renderToStaticMarkup(
      <AmbiguityBadge ambiguity={score(0.4, { success: 0.5 })} />,
    );
    expect(yellow).toContain("bg-status-awaiting/15");
    expect(yellow).toContain("완료 기준 불명확");

    const red = renderToStaticMarkup(<AmbiguityBadge ambiguity={score(0.7, { goal: 0.3 })} />);
    expect(red).toContain("bg-status-failed/15");
  });
});

describe("interviewStage", () => {
  it("채점이 시작되면 파기 상태와 무관하게 score, 아니면 파기 상태를 따른다", () => {
    expect(interviewStage(initialGrillState(), initialInterviewState())).toBe("idle");
    expect(interviewStage(grillAnswering, initialInterviewState())).toBe("dig");
    expect(interviewStage(grillDone, interviewAnswering)).toBe("score");
    expect(interviewStage(initialGrillState(), interviewDone)).toBe("score");
  });
});

describe("InterviewPanel — 진입", () => {
  it("idle에서는 깊게 파기와 채점만 두 진입점만 보이고 단계 표시는 없다", () => {
    const html = render({});
    expect(html).toContain("깊게 파기");
    expect(html).toContain("채점만");
    expect(html).not.toContain("1 깊게 파기");
    expect(html).not.toContain("답변 반영 — 점수 확정");
    expect(html).not.toContain("▸ 내 추천:");
  });

  it("깊게 파기 버튼은 1단계를, 채점만은 2단계를 바로 시작한다", () => {
    const onGrillStart = vi.fn();
    const onInterviewStart = vi.fn();
    const host = mount({ handlers: { onGrillStart, onInterviewStart } });
    clickButton(host, "깊게 파기");
    clickButton(host, "채점만");
    expect(onGrillStart).toHaveBeenCalledTimes(1);
    expect(onInterviewStart).toHaveBeenCalledTimes(1);
  });
});

describe("InterviewPanel — 1단계 깊게 파기", () => {
  it("단계 표시·라운드·질문·추천 답을 보여주고 객관식 보기는 렌더하지 않는다", () => {
    const html = render({ grill: grillAnswering });
    expect(html).toContain("1 깊게 파기");
    expect(html).toContain("라운드 1/11");
    expect(html).toContain("이 기능의 첫 사용자는 누구입니까");
    expect(html).toContain("1인 개발자입니다");
    expect(html).toContain("▸ 내 추천:");
    expect(html).toContain("<textarea");
    // 버튼은 헤더(다시 파기)·추천대로·모르겠다·답변·이만 종료 5개뿐 — 보기 목록이 아니다.
    expect((html.match(/<button/g) ?? []).length).toBe(5);
    expect(html).not.toContain("채점만");
  });

  it("남은 논점을 개수와 함께 노출하고 이만 종료를 부를 수 있다", () => {
    const onGrillEndNow = vi.fn();
    const host = mount({ grill: grillAnswering, handlers: { onGrillEndNow } });
    expect(host.innerHTML).toContain("남은 논점 2개");
    clickButton(host, "이만 종료");
    expect(onGrillEndNow).toHaveBeenCalledTimes(1);
  });

  it("진행 중에는 상태 문구와 비활성 버튼을 보인다", () => {
    const asking = render({ grill: { ...grillAnswering, phase: "asking", current: null } });
    expect(asking).toContain("질문 준비 중…");
    expect(asking).toContain("disabled");
    const noting = render({ grill: { ...grillAnswering, phase: "noting", current: null } });
    expect(noting).toContain("노트 정리 중…");
  });

  it("노트가 나오면 적용→채점이 주 경로이고 적용만·저장이 함께 보인다", () => {
    const onGrillApplyAndScore = vi.fn();
    const onGrillApplyInstruction = vi.fn();
    const host = mount({
      grill: grillDone,
      handlers: { onGrillApplyAndScore, onGrillApplyInstruction },
    });
    expect(host.innerHTML).toContain("무엇을 하려는가");
    expect(host.innerHTML).toContain("docs/explorations/에 저장");
    clickButton(host, "지시문에 적용 → 채점");
    clickButton(host, "적용만");
    expect(onGrillApplyAndScore).toHaveBeenCalledTimes(1);
    expect(onGrillApplyInstruction).toHaveBeenCalledTimes(1);
  });

  it("적용한 뒤에는 채점으로 넘어가는 버튼과 되돌리기가 남는다", () => {
    const onInterviewStart = vi.fn();
    const onUndo = vi.fn();
    const host = mount({
      grill: grillDone,
      currentInstruction: "새 지시문",
      onUndo,
      handlers: { onInterviewStart },
    });
    expect(host.innerHTML).not.toContain("지시문에 적용 → 채점");
    clickButton(host, "2단계: 채점 →");
    clickButton(host, "되돌리기");
    expect(onInterviewStart).toHaveBeenCalledTimes(1);
    expect(onUndo).toHaveBeenCalledTimes(1);
  });

  it("상한으로 끊긴 경우 쪼개기를 권한다", () => {
    const html = render({ grill: { ...grillDone, forcedEnd: true } });
    expect(html).toContain("11라운드 상한");
    expect(html).toContain("범위를 쪼개");
  });

  it("저장 후 경로를 표시한다", () => {
    const html = render({
      grill: { ...grillDone, savedPath: "docs/explorations/2026-08-12-note.md" },
    });
    expect(html).toContain("저장됨: docs/explorations/2026-08-12-note.md");
  });

  it("에러에도 작업 생성을 막지 않는다는 안내를 남긴다", () => {
    const html = render({
      grill: { ...grillAnswering, phase: "error", current: null, error: "CLI 없음" },
    });
    expect(html).toContain("CLI 없음");
    expect(html).toContain("작업 생성은 언제든 계속할 수 있습니다");
  });

  it("지시문이 바뀌면 신선도 경고를 띄운다", () => {
    expect(render({ grill: grillAnswering, currentInstruction: "바뀐 지시문" })).toContain(
      "변경되었습니다",
    );
    expect(render({ grill: grillAnswering })).not.toContain("변경되었습니다");
  });
});

describe("InterviewPanel — 2단계 채점", () => {
  it("assessing/crystallizing 진행 중에는 상태 문구와 비활성 버튼을 보인다", () => {
    const assessing = render({
      interview: { ...initialInterviewState(), phase: "assessing", instructionSnapshot: "지시문", repoSnapshot: "/repo" },
    });
    expect(assessing).toContain("2 채점");
    expect(assessing).toContain("지시문 분석 중…");
    expect(assessing).toContain("disabled");
    const crystallizing = render({
      interview: { ...interviewAnswering, phase: "crystallizing" },
    });
    expect(crystallizing).toContain("점수 확정 중…");
  });

  it("answering에서는 질문 카드(차원·이유·보기·스킵)와 점수 확정 버튼을 렌더한다", () => {
    const html = render({ interview: interviewAnswering });
    expect(html).toContain("목표");
    expect(html).toContain("무엇을 만드나요?");
    expect(html).toContain("목표가 불명확");
    expect(html).toContain(">A<");
    expect(html).toContain("답 없으면 건너뜀");
    expect(html).toContain("답변 반영 — 점수 확정");
    expect(html).toContain("모호성 0.60");
  });

  it("1단계 노트가 있으면 접힌 참고로 남고, 파기 질문·적용 버튼은 사라진다", () => {
    const html = render({ grill: grillDone, interview: interviewAnswering });
    expect(html).toContain("1단계 노트 — 깊게 파기 1라운드");
    expect(html).toContain("무엇을 하려는가");
    expect(html).toContain("docs/explorations/에 저장");
    expect(html).not.toContain("지시문에 적용 → 채점");
    expect(html).not.toContain("▸ 내 추천:");
  });

  it("done에서는 점수 확정 안내와 배지를 보인다", () => {
    const html = render({ interview: interviewDone });
    expect(html).toContain("모호성 점수가 확정되었습니다");
    expect(html).toContain("모호성 0.10");
    expect(html).toContain("다시 채점");
  });

  it("error에서는 재시도와 수동 편집 폴백 안내를 보인다", () => {
    const html = render({
      interview: { ...interviewAnswering, phase: "error", error: "타임아웃" },
    });
    expect(html).toContain("타임아웃");
    expect(html).toContain("재시도");
    expect(html).toContain("수동 편집으로 계속할 수 있습니다");
  });

  it("지시문 또는 레포가 바뀌면 채점 기준의 신선도 경고를 보인다", () => {
    const grillFresh = { ...grillDone, instructionSnapshot: "바뀐 지시문" };
    // 파기 스냅샷은 낡았어도 채점 스냅샷이 현재 입력과 같으면 경고하지 않는다.
    expect(render({ grill: grillFresh, interview: interviewDone })).not.toContain("변경되었습니다");
    expect(render({ interview: interviewDone, currentInstruction: "바뀐 지시문" })).toContain(
      "다시 채점하세요",
    );
    expect(render({ interview: interviewDone, currentRepo: "/other-repo" })).toContain(
      "변경되었습니다",
    );
  });
});

describe("닫기(✕)", () => {
  it("진행 중에만 그려진다 — idle이거나 onClose가 없으면 없다", () => {
    expect(render({ grill: grillAnswering, onClose: () => {} })).toContain('aria-label="닫기"');
    expect(render({ interview: interviewAnswering, onClose: () => {} })).toContain(
      'aria-label="닫기"',
    );
    expect(render({ onClose: () => {} })).not.toContain('aria-label="닫기"');
    expect(render({ grill: grillAnswering })).not.toContain('aria-label="닫기"');
  });

  it("누르면 onClose를 부른다", () => {
    const onClose = vi.fn();
    const host = mount({ grill: grillDone, interview: interviewAnswering, onClose });
    const button = host.querySelector('[aria-label="닫기"]') as HTMLButtonElement;
    act(() => button.click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
