// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VendorUsage } from "../../lib/ipc";

const mocks = vi.hoisted(() => ({
  usageClaudeTokenSet: vi.fn(),
  usageClaudeTokenClear: vi.fn(),
  usageClaudeTokenStatus: vi.fn(),
  usageSnapshot: vi.fn(),
}));

vi.mock("../../lib/ipc", () => ({
  usageClaudeTokenSet: mocks.usageClaudeTokenSet,
  usageClaudeTokenClear: mocks.usageClaudeTokenClear,
  usageClaudeTokenStatus: mocks.usageClaudeTokenStatus,
  usageSnapshot: mocks.usageSnapshot,
}));

import { UsageTokenSection } from "./UsageTokenSection";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const usage = (): VendorUsage => ({
  vendor: "claude",
  label: "Claude Code",
  status: "ok",
  detail: null,
  plan: null,
  five_hour: { used_percent: 10, resets_at: null, window_minutes: 300 },
  weekly: null,
  source: "manual-token",
  updated_at: 1000,
});

let container: HTMLDivElement;
let root: Root;

const field = () => container.querySelector("input") as HTMLInputElement;
const button = (text: string) =>
  [...container.querySelectorAll("button")].find((b) => b.textContent === text) as HTMLButtonElement;

async function mount() {
  await act(async () => {
    root.render(<UsageTokenSection />);
  });
}

async function type(value: string) {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    )?.set;
    setter?.call(field(), value);
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.usageClaudeTokenStatus.mockResolvedValue(false);
  mocks.usageClaudeTokenSet.mockResolvedValue(usage());
  mocks.usageClaudeTokenClear.mockResolvedValue(undefined);
  mocks.usageSnapshot.mockResolvedValue({ vendors: [], fetched_at: 0 });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("UsageTokenSection", () => {
  it("검증에 통과한 토큰만 저장으로 표시하고 사용량을 즉시 새로고침한다", async () => {
    await mount();
    await type("sk-ant-oat-test");
    await act(async () => button("저장하고 확인").click());

    expect(mocks.usageClaudeTokenSet).toHaveBeenCalledWith("sk-ant-oat-test");
    expect(mocks.usageSnapshot).toHaveBeenCalledWith(true);
    expect(container.textContent).toContain("저장됨");
    // 입력값이 화면에 남지 않는다 — 토큰은 저장 뒤 어디에도 보이지 않는다.
    expect(field().value).toBe("");
  });

  it("거절된 토큰은 저장되지 않고 사유만 남는다", async () => {
    mocks.usageClaudeTokenSet.mockRejectedValue(
      "이 토큰으로는 사용량을 조회할 수 없습니다(HTTP 403)",
    );
    await mount();
    await type("bad-token");
    await act(async () => button("저장하고 확인").click());

    expect(container.textContent).toContain("HTTP 403");
    expect(container.textContent).toContain("저장된 토큰 없음");
    expect(container.textContent).not.toContain("bad-token");
  });

  it("저장된 토큰이 없으면 지우기를 누를 수 없다", async () => {
    await mount();
    expect(button("지우기").disabled).toBe(true);
  });
});
