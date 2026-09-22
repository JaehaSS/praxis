// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  quizAvailability: vi.fn(),
}));

vi.mock("../../lib/ipc", () => ({ quizAvailability: mocks.quizAvailability }));
// 패널은 자기 IPC를 직접 부른다 — 카드가 무엇을 여는지만 보면 되므로 표식으로 갈음한다.
vi.mock("../QuizPanel", () => ({ QuizPanel: () => <div data-panel="quiz" /> }));
vi.mock("../QuizReviewPanel", () => ({ QuizReviewPanel: () => <div data-panel="review" /> }));

import { HOME_QUIZ_POLL_MS, HomeQuizCard } from "./HomeQuizCard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const card = () => container?.querySelector("button");
const panel = () => container?.querySelector("[data-panel]")?.getAttribute("data-panel");

async function render() {
  await act(async () => {
    root?.render(<HomeQuizCard />);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  mocks.quizAvailability.mockResolvedValue({ askable: 0, pending_review: 0 });
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

describe("HomeQuizCard", () => {
  it("낼 것이 없으면 자리를 차지하지 않는다", async () => {
    await render();

    // ADR 0114와 같은 규칙 — 빈 카드가 시키는 일(스케줄 등록)은 홈에서 할 일이 아니다.
    expect(container?.textContent).toBe("");
  });

  it("풀 문제가 쌓이면 개수와 함께 뜬다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 3, pending_review: 2 });

    await render();

    expect(container?.textContent).toContain("풀 문제가 쌓여 있습니다");
    expect(container?.textContent).toContain("3문제");
    expect(container?.textContent).toContain("검수 2");
  });

  it("출제가 0이고 검수만 남으면 검수로 안내한다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 0, pending_review: 1 });

    await render();

    expect(container?.textContent).toContain("검수를 기다리는 문제가 있습니다");
  });

  it("대기 없이도 눌러서 연다 — 이것이 이 카드를 만든 이유다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 1, pending_review: 0 });
    await render();
    expect(panel()).toBeUndefined();

    await act(async () => card()?.click());

    expect(panel()).toBe("quiz");
  });

  it("검수만 있으면 검수 화면을 연다", async () => {
    mocks.quizAvailability.mockResolvedValue({ askable: 0, pending_review: 4 });
    await render();

    await act(async () => card()?.click());

    expect(panel()).toBe("review");
  });

  it("폴링이 큐 변화를 따라잡는다", async () => {
    await render();
    expect(container?.textContent).toBe("");

    mocks.quizAvailability.mockResolvedValue({ askable: 1, pending_review: 0 });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(HOME_QUIZ_POLL_MS);
    });

    expect(container?.textContent).toContain("1문제");
  });
});
