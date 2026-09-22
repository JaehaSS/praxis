// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
vi.mock("../ThemeEditor", () => ({ ThemeEditor: () => null }));
import { ThemeSection } from "./ThemeSection";
import { applyTheme, getActiveTheme, loadThemeId, THEMES } from "../../../lib/themes";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let container: HTMLDivElement;
let root: Root;
beforeEach(async () => {
  localStorage.clear();
  applyTheme("praxis-dark");
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => root.render(<ThemeSection />));
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});
const button = (label: string) => [...container.querySelectorAll("button")].find((b) => b.textContent === label || [...b.querySelectorAll("span")].some((s) => s.textContent === label))!;

it("밝기 필터는 해당 테마만 보여주고 현재 선택은 유지한다", async () => {
  await act(async () => button("라이트").click());
  for (const theme of THEMES) {
    expect(Boolean(button(theme.label)), theme.id).toBe(theme.kind === "light");
  }
  expect(getActiveTheme().id).toBe("praxis-dark");
  await act(async () => button("다크").click());
  for (const theme of THEMES) {
    expect(Boolean(button(theme.label)), theme.id).toBe(theme.kind === "dark");
  }
  await act(async () => button("전체").click());
  for (const theme of THEMES) expect(button(theme.label)).toBeTruthy();
});

it("새 테마 카드는 즉시 적용·저장되고 선택 상태를 표시한다", async () => {
  for (const id of ["cream", "coffee", "sakura", "plum", "mint", "forest", "lavender", "midnight"]) {
    const theme = THEMES.find((t) => t.id === id)!;
    await act(async () => button(theme.label).click());
    expect(getActiveTheme().id).toBe(id);
    expect(loadThemeId()).toBe(id);
    expect(document.documentElement.style.getPropertyValue("--c-bg")).toBe(theme.tokens.bg);
    expect(button(theme.label).getAttribute("aria-pressed")).toBe("true");
    expect(button(theme.label).textContent).toContain("적용 중");
  }
});
