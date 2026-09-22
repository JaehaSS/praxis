// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskActivity: vi.fn(),
  quizAvailability: vi.fn(),
  insightAvailability: vi.fn(),
}));

vi.mock("./ipc", () => ({
  taskActivity: mocks.taskActivity,
  quizAvailability: mocks.quizAvailability,
  insightAvailability: mocks.insightAvailability,
}));

import { QUIZ_GATE_POLL_MS, useQuizGate } from "./use-quiz-gate";

/** 지식창고가 연결되지 않은 가용성 — 게이트는 `cards` 합계만 본다. */
const WIKI_NONE = { deck_cards: 0, wiki_notes: 0, wiki_cards: 0, wiki_root: null };

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** 임계값을 한참 넘긴 대기 하나. `started_at`은 초 단위다. */
const longWait = [{ task_id: 1, last_operation: null, last_event_at: 0, started_at: 0 }];

function Probe({ active, taskId = null }: { active: boolean; taskId?: number | null }) {
  const gate = useQuizGate(active, taskId);
  return <div data-mode={gate.mode ?? "none"} data-pending={gate.pendingReview} />;
}

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const mode = () => container?.firstElementChild?.getAttribute("data-mode");

async function render(active: boolean, taskId: number | null = null) {
  await act(async () => {
    root?.render(<Probe active={active} taskId={taskId} />);
  });
}

/** 폴링 한 바퀴 — 타이머를 돌리고 그 안의 await까지 흘려보낸다. */
async function tick() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(QUIZ_GATE_POLL_MS);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  mocks.taskActivity.mockResolvedValue(longWait);
  mocks.quizAvailability.mockResolvedValue({ askable: 0, pending_review: 0 });
  mocks.insightAvailability.mockResolvedValue({ ...WIKI_NONE, cards: 0, decks: 0, enabled: true, warnings: [] });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("useQuizGate", () => {
  it("큐가 비면 대기가 아무리 길어도 열지 않는다", async () => {
    await render(true);
    await tick();
    await tick();

    // 이 한 줄이 이슈 #87이다 — 전에는 경과만 보고 열어 "문제가 없습니다"를 반복했다.
    expect(mode()).toBe("none");
  });

  it("낼 문제가 있으면 퀴즈로 연다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 2, pending_review: 0 });

    await render(true);

    expect(mode()).toBe("quiz");
  });

  it("낼 것이 없고 검수만 남았으면 검수로 연다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 0, pending_review: 3 });

    await render(true);

    expect(mode()).toBe("review");
    expect(container?.firstElementChild?.getAttribute("data-pending")).toBe("3");
  });

  it("대기가 짧으면 큐를 조회조차 하지 않는다", async () => {
    const justStarted = [{ ...longWait[0], started_at: Math.floor(Date.now() / 1000) }];
    mocks.taskActivity.mockResolvedValue(justStarted);

    await render(true);
    await tick();

    expect(mocks.quizAvailability).not.toHaveBeenCalled();
    expect(mode()).toBe("none");
  });

  it("한 번 열리면 큐가 바뀌어도 화면이 바뀌지 않는다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 1, pending_review: 4 });
    await render(true);
    expect(mode()).toBe("quiz");
    const callsWhenOpened = mocks.quizAvailability.mock.calls.length;

    // 마지막 문제를 풀어 낼 것이 없어졌다 — 그래도 풀던 화면이 검수로 튀면 안 된다.
    mocks.quizAvailability.mockResolvedValue({ askable: 0, pending_review: 4 });
    await tick();

    expect(mode()).toBe("quiz");
    expect(mocks.quizAvailability.mock.calls.length).toBe(callsWhenOpened);
  });

  it("응답이 도착해도 닫지 않고, 다음 대기가 시작되면 다시 판정한다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 1, pending_review: 0 });
    await render(true);
    expect(mode()).toBe("quiz");

    // 턴이 끝났다 — 풀던 문제는 그 자리에 남는다(DR-3).
    await render(false);
    expect(mode()).toBe("quiz");

    // 다음 턴이 시작되면 지난 대기의 화면을 지우고 처음부터 잰다.
    mocks.quizAvailability.mockResolvedValue({ askable: 0, pending_review: 0 });
    await render(true);
    expect(mode()).toBe("none");
    expect(mocks.quizAvailability).toHaveBeenCalledTimes(2);
  });

  it("작업 id를 주면 그 작업의 대기만 잰다 — 다른 세션이 오래 기다려도 열지 않는다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 1, pending_review: 0 });
    mocks.taskActivity.mockResolvedValue([{ task_id: 2, last_operation: null, last_event_at: 0, started_at: 0 }]);
    await render(true, 1);
    expect(mode()).toBe("none");
    expect(mocks.quizAvailability).not.toHaveBeenCalled();

    mocks.taskActivity.mockResolvedValue([
      { task_id: 2, last_operation: null, last_event_at: 0, started_at: 0 },
      { task_id: 1, last_operation: null, last_event_at: 0, started_at: 0 },
    ]);
    await tick();
    expect(mode()).toBe("quiz");
  });

  it("조회가 실패하면 열지 않는다", async () => {
    mocks.quizAvailability.mockRejectedValue(new Error("db down"));

    await render(true);
    await tick();

    expect(mode()).toBe("none");
  });
});

describe("useQuizGate — 인사이트", () => {
  it("퀴즈도 복습도 없고 인사이트만 있으면 인사이트로 연다", async () => {
    mocks.insightAvailability.mockResolvedValue({ ...WIKI_NONE, cards: 3, decks: 1, enabled: true, warnings: [] });
    await render(true);
    await tick();
    expect(mode()).toBe("insight");
  });

  it("퀴즈가 있으면 인사이트보다 퀴즈가 먼저다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 2, pending_review: 0 });
    mocks.insightAvailability.mockResolvedValue({ ...WIKI_NONE, cards: 9, decks: 2, enabled: true, warnings: [] });
    await render(true);
    await tick();
    expect(mode()).toBe("quiz");
  });

  it("인사이트 조회가 실패해도 퀴즈는 뜬다", async () => {
    // 새 소스가 기존 경로를 막으면 안 된다 — 각자 폴백을 갖는 이유다.
    mocks.insightAvailability.mockRejectedValue(new Error("boom"));
    mocks.quizAvailability.mockResolvedValue({ askable: 2, pending_review: 0 });
    await render(true);
    await tick();
    expect(mode()).toBe("quiz");
  });

  it("셋 다 없으면 열지 않는다", async () => {
    await render(true);
    await tick();
    expect(mode()).toBe("none");
  });

  it("꺼져 있으면 카드가 있어도 띄우지 않는다", async () => {
    // 가용성은 현실을 보고하고(카드 5장) 판단은 게이트가 한다.
    mocks.insightAvailability.mockResolvedValue({
      ...WIKI_NONE,
      cards: 5,
      decks: 1,
      enabled: false,
      warnings: [],
    });
    await render(true);
    await tick();
    expect(mode()).toBe("none");
  });
});
