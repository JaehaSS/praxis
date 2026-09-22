// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  quizNext: vi.fn(),
  quizAnswer: vi.fn(),
  quizReport: vi.fn(),
}));

vi.mock("../lib/ipc", () => ({
  quizNext: mocks.quizNext,
  quizAnswer: mocks.quizAnswer,
  quizReport: mocks.quizReport,
}));

import { QuizPanel } from "./QuizPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const item = {
  id: 7,
  kind: "vocab",
  question: "롤백 절차는?",
  choices: ["태그", "리셋"],
  source_excerpt: null,
};

let container: HTMLDivElement | null = null;
let root: Root | null = null;
const onClose = vi.fn();
const onReview = vi.fn();

const text = () => container?.textContent ?? "";
const button = (label: string) =>
  [...(container?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.trim() === label);

async function render(pendingReview = 0) {
  await act(async () => {
    root?.render(
      <QuizPanel
        responseArrived={false}
        pendingReview={pendingReview}
        onClose={onClose}
        onReview={onReview}
      />,
    );
  });
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  vi.clearAllMocks();
});

describe("QuizPanel", () => {
  it("낼 문제가 없으면 안내 대신 조용히 닫는다", async () => {
    mocks.quizNext.mockResolvedValue(null);

    await render();

    // 게이트가 걸러 주므로 여기 오는 것은 드문 경쟁이다. 그때 할 말이 없으니 닫는다.
    expect(onClose).toHaveBeenCalled();
    expect(text()).not.toContain("출제할 문제가 없습니다");
  });

  it("풀다가 소진되면 닫지 않고 다 풀었다고 알린다", async () => {
    mocks.quizNext.mockResolvedValueOnce(item).mockResolvedValue(null);
    mocks.quizAnswer.mockResolvedValue({ correct: true, answer: "태그", explanation: null });
    await render();

    await act(async () => button("태그")?.click());
    await act(async () => button("다음 문제")?.click());

    expect(text()).toContain("준비된 문제를 다 풀었습니다");
    // 처음부터 없던 것과 다르다 — 방금 푼 사람에게 화면이 사라지면 결과를 못 본다.
    expect(onClose).not.toHaveBeenCalled();
  });

  it("검수 대기가 없으면 통로를 보이지 않는다", async () => {
    mocks.quizNext.mockResolvedValue(item);

    await render(0);

    // 눌러 봐야 빈 목록이다. 통로가 필요해지면 게이트가 검수 화면으로 직접 연다.
    expect(text()).not.toContain("검수");
  });

  it("검수 대기가 있으면 건수와 함께 통로를 연다", async () => {
    mocks.quizNext.mockResolvedValue(item);

    await render(3);
    await act(async () => button("검수 3")?.click());

    expect(onReview).toHaveBeenCalled();
  });
});
