// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  list: vi.fn(),
}));

vi.mock("../../../lib/ipc", () => ({
  retroDigestGet: mocks.get,
  retroDigestList: mocks.list,
}));

import { RetroPanel } from "./RetroPanel";
import type { RetroDigest } from "../../../lib/ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** 2026-08-24(월) 00:00 KST. */
const THIS_WEEK = 1_787_497_200;
const LAST_WEEK = THIS_WEEK - 7 * 86_400;

const digest = (weekStart: number, body: string): RetroDigest => ({
  week_start: weekStart,
  body,
  facts: {
    week_start: weekStart,
    tasks_total: 23,
    tasks_done: 17,
    tasks_discarded: 4,
    discard_rate_pct: 17.4,
    discard_rate_prev_pct: 24,
    followup_pct: 58,
    proposals_pending: 762,
    proposals_applied: 0,
    top_role: null,
  },
  agent: "claude",
  model: null,
  generated_at: weekStart,
});

const CURRENT = digest(THIS_WEEK, "이번 주 서술이다. 승인이 늘었고 폐기는 줄었다.");
const PREVIOUS = digest(LAST_WEEK, "지난주 서술이다. 폐기가 눈에 띄게 많았다.");

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  mocks.get
    .mockReset()
    .mockImplementation((week: number | null) =>
      Promise.resolve(week === LAST_WEEK ? PREVIOUS : CURRENT),
    );
  mocks.list.mockReset().mockResolvedValue([
    { week_start: THIS_WEEK, generated_at: THIS_WEEK },
    { week_start: LAST_WEEK, generated_at: LAST_WEEK },
  ]);
  localStorage.clear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

async function mount(initial: RetroDigest | null) {
  await act(async () => {
    root.render(<RetroPanel initial={initial} />);
  });
}

function clickButton(label: string) {
  const button = [...container.querySelectorAll("button")].find((b) =>
    b.textContent?.includes(label),
  );
  if (!button) throw new Error(`버튼을 찾지 못했습니다: ${label}`);
  act(() => button.dispatchEvent(new MouseEvent("click", { bubbles: true })));
}

describe("RetroPanel", () => {
  it("상위가 준 다이제스트를 다시 조회하지 않고 그대로 쓴다", async () => {
    await mount(CURRENT);

    expect(container.textContent).toContain("이번 주 서술이다");
    expect(mocks.get).not.toHaveBeenCalled();
  });

  it("최신 주로 되돌아오면 화면도 최신 주로 돌아온다", async () => {
    // 되돌아올 때 그냥 빠져나가면 화면이 지난주에 머문 채로 남는다.
    await mount(CURRENT);

    await act(async () => {
      clickButton("지난주");
    });
    expect(container.textContent).toContain("지난주 서술이다");

    await act(async () => {
      clickButton("다음주");
    });
    expect(container.textContent).toContain("이번 주 서술이다");
    expect(container.textContent).not.toContain("지난주 서술이다");
  });

  it("주 목록을 못 읽으면 이동을 잠그고 최신 주를 지킨다", async () => {
    // 목록 조회가 실패해도 화면이 비지 않는다 — 상위가 준 최신 주는 그대로 남는다.
    mocks.list.mockResolvedValue([]);
    await mount(CURRENT);

    expect(container.textContent).toContain("이번 주 서술이다");
    const nav = [...container.querySelectorAll("button")].filter((b) =>
      /지난주|다음주/.test(b.textContent ?? ""),
    );
    expect(nav).toHaveLength(2);
    expect(nav.every((b) => b.hasAttribute("disabled"))).toBe(true);
  });

  it("적체를 순화하지 않고 그대로 쓴다", async () => {
    // "검토 대기 3건"처럼 보여주면 이 섹션의 존재 이유가 사라진다(설계 0054 §6.4).
    await mount(CURRENT);

    expect(container.textContent).toContain("762");
    expect(container.textContent).toContain("쌓이기만 하고 하나도 채택되지 않았습니다");
  });

  it("적체가 있으면 승인의 유일한 자리로 보낸다", async () => {
    // 승인·거부는 메모리 › 자기개선 탭에서만 한다(ADR 0191) — 여기 남는 것은 링크뿐이다.
    const open = vi.fn();
    await act(async () => {
      root.render(<RetroPanel initial={CURRENT} onOpenSelfImprove={open} />);
    });

    clickButton("자기개선에서 검토");
    expect(open).toHaveBeenCalled();
  });

  it("적체가 없으면 검토 링크를 띄우지 않는다", async () => {
    const empty = digest(THIS_WEEK, "이번 주 서술이다.");
    empty.facts.proposals_pending = 0;
    await act(async () => {
      root.render(<RetroPanel initial={empty} onOpenSelfImprove={vi.fn()} />);
    });

    expect(container.textContent).not.toContain("자기개선에서 검토");
  });

  it("회고가 없으면 스케줄 화면을 안내한다", async () => {
    await mount(null);

    expect(container.textContent).toContain("아직 생성된 회고가 없습니다");
    expect(container.textContent).toContain("주간 회고");
  });

  it("최신 주를 열면 읽음으로 남긴다", async () => {
    const seen = vi.fn();
    await act(async () => {
      root.render(<RetroPanel initial={CURRENT} onSeen={seen} />);
    });

    expect(seen).toHaveBeenCalledWith(THIS_WEEK);
    expect(localStorage.getItem("praxis:retro-seen")).toBe(String(THIS_WEEK));
  });
});
