// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TaskVendorReview } from "./TaskVendorReview";
import { multiReview, reviewDelete, reviewGet, reviewHistoryList } from "../../lib/ipc";
import type { MultiReviewResult, ReviewDetail, ReviewMeta } from "../../lib/ipc";

vi.mock("../../lib/ipc", () => ({
  multiReview: vi.fn(),
  reviewHistoryList: vi.fn(),
  reviewGet: vi.fn(),
  reviewDelete: vi.fn(),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const meta = (over: Partial<ReviewMeta> = {}): ReviewMeta => ({
  id: 11,
  created_at: 1751780280,
  repo: "/workspace/praxis",
  source_kind: "diff",
  source_ref: "7",
  focus: "",
  ok_count: 2,
  total: 3,
  ...over,
});

const result = (text: string): MultiReviewResult => ({
  items: [{ vendor: "claude", ok: true, text }],
  synthesis: null,
});

const detail: ReviewDetail = {
  content: "diff",
  prompt_review: "p",
  prompt_synthesis: null,
  model_info: [],
  synthesis_model: null,
};

let container: HTMLDivElement | null = null;
let root: Root | null = null;

async function render(host = "local") {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root?.render(<TaskVendorReview repo="/workspace/praxis" taskId={7} host={host} />);
  });
  return container;
}

/** 텍스트로 버튼을 찾는다 — 클래스나 순서에 기대지 않는다. */
const button = (el: HTMLElement, text: string): HTMLButtonElement | undefined =>
  [...el.querySelectorAll("button")].find((b) => b.textContent?.includes(text));

const click = async (el: Element | undefined) => {
  await act(async () => {
    el?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
};

beforeEach(() => {
  vi.mocked(reviewHistoryList).mockResolvedValue([
    meta(),
    // 다른 작업·다른 종류의 리뷰는 이 작업의 이력이 아니다.
    meta({ id: 12, source_ref: "9" }),
    meta({ id: 13, source_kind: "plan", source_ref: "7" }),
  ]);
  vi.mocked(multiReview).mockResolvedValue({ result: result("실행 결과"), detail });
  vi.mocked(reviewGet).mockResolvedValue({ meta: meta(), result: result("이력 본문"), detail });
  vi.mocked(reviewDelete).mockResolvedValue(undefined);
});

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  container = null;
  root = null;
  vi.clearAllMocks();
});

describe("TaskVendorReview", () => {
  it("이 작업의 리뷰 건수를 버튼 배지로 센다", async () => {
    const el = await render();

    expect(button(el, "벤더 리뷰")?.textContent).toBe("벤더 리뷰 1");
  });

  it("실행하면 이 작업의 diff를 source_ref로 넘긴다", async () => {
    const el = await render();

    await click(button(el, "벤더 리뷰"));
    await click(button(el, "실행"));

    expect(multiReview).toHaveBeenCalledWith(
      "/workspace/praxis",
      "diff",
      "7",
      "",
      ["claude", "codex", "agy"],
      true,
    );
    expect(el.textContent).toContain("실행 결과");
  });

  it("이력 한 건을 누르면 그 결과 본문을 패널에 띄운다", async () => {
    const el = await render();

    await click(button(el, "벤더 리뷰"));
    await click(button(el, "2/3 완료"));

    expect(reviewGet).toHaveBeenCalledWith(11);
    expect(el.textContent).toContain("이력 본문");
  });

  it("원격 호스트 작업에서는 아무것도 그리지 않는다", async () => {
    const el = await render("remote");

    expect(el.innerHTML).toBe("");
    expect(reviewHistoryList).not.toHaveBeenCalled();
  });
});
