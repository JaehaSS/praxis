// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const insightNext = vi.fn();
vi.mock("../lib/ipc", () => ({ insightNext: (...a: unknown[]) => insightNext(...a) }));

import { InsightCard } from "./InsightCard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CARD = {
  key: "insurance#손해율",
  deck: "보험 도메인",
  title: "손해율과 합산비율은 다른 것을 잰다",
  body: "발생손해액 ÷ 경과보험료가 손해율이다.",
  source: "보험업감독업무시행세칙 별표",
  tags: ["손해보험"],
};

let container: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  insightNext.mockReset();
  insightNext.mockResolvedValue(CARD);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

const render = async (onClose = () => undefined) =>
  act(async () => root?.render(<InsightCard onClose={onClose} />));

const byText = (t: string) =>
  [...(container?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.includes(t));

describe("InsightCard", () => {
  it("제목·본문·출처를 모두 보여준다", async () => {
    await render();
    const text = container?.textContent ?? "";
    expect(text).toContain(CARD.title);
    expect(text).toContain(CARD.body);
    expect(text).toContain(CARD.source);
  });

  it("출처가 접히지 않는다 — 클릭 없이 바로 보인다", async () => {
    await render();
    // 접으면 아무도 펴지 않고, 그러면 출처를 필수로 만든 이유가 사라진다.
    expect(container?.querySelector("details")).toBeNull();
    expect(container?.textContent).toContain("출처 ·");
  });

  it("덱 이름을 분류 태그로 보여준다", async () => {
    await render();
    expect(container?.textContent).toContain(CARD.deck);
  });

  it("다음을 누르면 새 카드를 요청한다", async () => {
    await render();
    expect(insightNext).toHaveBeenCalledTimes(1);
    await act(async () => byText("다음")?.click());
    expect(insightNext).toHaveBeenCalledTimes(2);
  });

  it("카드가 없으면 그 사실을 말한다", async () => {
    insightNext.mockResolvedValue(null);
    await render();
    expect(container?.textContent).toContain("띄울 카드가 없습니다");
  });

  it("조회가 실패해도 화면이 깨지지 않는다", async () => {
    insightNext.mockRejectedValue(new Error("boom"));
    await render();
    expect(container?.textContent).toContain("불러오지 못했습니다");
  });

  it("닫기 버튼이 onClose를 부른다", async () => {
    const onClose = vi.fn();
    await render(onClose);
    await act(async () =>
      container?.querySelector<HTMLButtonElement>('button[aria-label="인사이트 닫기"]')?.click(),
    );
    expect(onClose).toHaveBeenCalled();
  });
});
