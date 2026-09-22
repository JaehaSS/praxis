// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyTheme, DEFAULT_THEME_ID } from "../../lib/themes";

const mocks = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn(),
  writeText: vi.fn(),
}));

vi.mock("mermaid", () => ({
  default: { initialize: mocks.initialize, render: mocks.render },
}));

import { Markdown } from "./Markdown";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  applyTheme(DEFAULT_THEME_ID);
  mocks.initialize.mockClear();
  mocks.render.mockReset();
  mocks.render.mockResolvedValue({ svg: '<svg data-testid="mermaid-svg"></svg>' });
  mocks.writeText.mockReset();
  mocks.writeText.mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: mocks.writeText },
    configurable: true,
  });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  applyTheme(DEFAULT_THEME_ID);
});

const render = (text: string) =>
  act(async () => {
    root.render(<Markdown text={text} />);
    await new Promise((resolve) => setTimeout(resolve));
  });

describe("session Mermaid fences", () => {
  it("can server-render a Mermaid fence", () => {
    expect(() => renderToStaticMarkup(<Markdown text="```mermaid\nflowchart TD\nA --> B\n```" />)).not.toThrow();
  });

  it("renders a case-insensitive Mermaid fence with the active theme and source copy", async () => {
    const chart = "flowchart TD\nA[시작] --> B[끝]\n";
    await render(`\`\`\`MERMAID\n${chart}\`\`\``);

    expect(host.querySelector('[data-testid="mermaid-svg"]')).not.toBeNull();

    expect(mocks.render).toHaveBeenCalledWith(expect.stringMatching(/^mmd-/), chart);
    expect(mocks.initialize).toHaveBeenCalledWith(expect.objectContaining({ theme: "dark", securityLevel: "strict" }));

    await act(async () => {
      applyTheme("praxis-light");
      await new Promise((resolve) => setTimeout(resolve));
    });
    expect(mocks.initialize).toHaveBeenLastCalledWith(expect.objectContaining({ theme: "default" }));

    await act(async () => {
      host.querySelector<HTMLButtonElement>('button[aria-label="코드 복사"]')!.click();
      await Promise.resolve();
    });
    expect(mocks.writeText).toHaveBeenCalledWith(chart);
  });

  it("keeps ordinary fenced code on the existing code and copy path", async () => {
    await render("```typescript\nconst answer = 42;\n```");

    expect(host.textContent).toContain("typescript");
    expect(host.textContent).toContain("const answer = 42;");
    expect(host.querySelector('button[aria-label="코드 복사"]')).not.toBeNull();
    expect(mocks.render).not.toHaveBeenCalled();
  });

  it("falls back to readable source for an incomplete chart and recovers when streaming completes", async () => {
    mocks.render.mockImplementation((_id: string, chart: string) =>
      chart.includes("미완성")
        ? Promise.reject(new Error("syntax error"))
        : Promise.resolve({ svg: '<svg data-testid="mermaid-svg"></svg>' }),
    );
    await render("```mermaid\nflowchart TD\n미완성\n```");

    expect(host.textContent).toContain("Mermaid 렌더 실패: syntax error");
    expect(host.textContent).toContain("미완성");

    await render("```mermaid\nflowchart TD\nA[시작] --> B[끝]\n```");

    expect(host.querySelector('[data-testid="mermaid-svg"]')).not.toBeNull();
    expect(host.textContent).not.toContain("Mermaid 렌더 실패");
  });
});
