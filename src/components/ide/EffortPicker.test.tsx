// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EffortPicker } from "./EffortPicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
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

const openMenu = async () => {
  const chip = container?.querySelector<HTMLButtonElement>('button[aria-label="Reasoning effort"]');
  expect(chip).toBeTruthy();
  await act(async () => chip?.click());
};

describe("EffortPicker", () => {
  it("칩은 현재 오버라이드를 표시하고, 메뉴에 기본 항목과 Sol의 모든 effort를 나열한다", async () => {
    await act(async () =>
      root?.render(
        <EffortPicker agent="codex" model="gpt-5.6-sol" effort="high" onChange={() => undefined} />,
      ),
    );

    expect(container?.textContent).toContain("Effort: high");
    await openMenu();

    const text = container?.textContent ?? "";
    expect(text).toContain("기본 (설정값)");
    expect(text).toContain("max");
    expect(text).toContain("ultra");
  });

  it("선택한 모델이 지원하지 않는 effort는 메뉴에서 제외한다", async () => {
    await act(async () =>
      root?.render(
        <EffortPicker agent="codex" model="gpt-5.6-luna" effort="max" onChange={() => undefined} />,
      ),
    );
    await openMenu();

    const text = container?.textContent ?? "";
    expect(text).toContain("max");
    expect(text).not.toContain("ultra");
  });

  it("claude reasoning effort는 max까지만 — codex 전용 ultra는 없다", async () => {
    await act(async () =>
      root?.render(<EffortPicker agent="claude" model="opus" effort="high" onChange={() => undefined} />),
    );
    await openMenu();

    const text = container?.textContent ?? "";
    expect(text).toContain("max");
    expect(text).not.toContain("ultra");
  });

  it("Antigravity 별칭은 기본값과 low, medium, high만 표시한다", async () => {
    await act(async () =>
      root?.render(<EffortPicker agent="gemini" model="gemini-3-pro" effort="" onChange={() => undefined} />),
    );
    await openMenu();

    const text = container?.textContent ?? "";
    expect(text).toContain("기본 (설정값)");
    expect(text).toContain("low");
    expect(text).toContain("medium");
    expect(text).toContain("high");
    expect(text).not.toContain("xhigh");
    expect(text).not.toContain("max");
  });

  it("항목을 고르면 onChange 후 메뉴가 닫히고, 기본 항목은 빈 문자열을 전달한다", async () => {
    const onChange = vi.fn();
    await act(async () =>
      root?.render(
        <EffortPicker agent="codex" model="gpt-5.6-sol" effort="high" onChange={onChange} />,
      ),
    );
    await openMenu();

    const items = Array.from(container?.querySelectorAll<HTMLButtonElement>("button") ?? []);
    const ultra = items.find((el) => el.textContent?.trim() === "ultra");
    expect(ultra).toBeTruthy();
    await act(async () => ultra?.click());
    expect(onChange).toHaveBeenCalledWith("ultra");
    expect(container?.textContent).not.toContain("기본 (설정값)");

    await openMenu();
    const defaultItem = Array.from(
      container?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((el) => el.textContent?.includes("기본 (설정값)"));
    await act(async () => defaultItem?.click());
    expect(onChange).toHaveBeenCalledWith("");
  });
});
