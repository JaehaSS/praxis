// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Markdown } from "./Markdown";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
const writeText = vi.fn(() => Promise.resolve());

beforeEach(() => {
  vi.useFakeTimers();
  writeText.mockClear();
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const render = (text: string) => act(() => root.render(<Markdown text={text} />));
const copyButton = () => host.querySelector<HTMLButtonElement>('button[aria-label="코드 복사"]');

describe("펜스 코드블록 복사 버튼", () => {
  it("블록의 원본 텍스트를 클립보드에 넣는다", async () => {
    render("```cron\n5 0 * * * echo hi\n```");

    const button = copyButton();
    expect(button).not.toBeNull();
    await act(async () => button!.click());

    expect(writeText).toHaveBeenCalledWith("5 0 * * * echo hi\n");
  });

  it("복사 직후 라벨이 바뀌고 잠시 뒤 되돌아온다", async () => {
    render("```\nnpm run tauri build\n```");

    await act(async () => copyButton()!.click());
    expect(host.querySelector('button[aria-label="복사됨"]')).not.toBeNull();

    await act(async () => {
      vi.advanceTimersByTime(1500);
    });
    expect(copyButton()).not.toBeNull();
  });

  it("언어 라벨이 없는 블록에도 버튼이 있다", () => {
    render("```\nplain\n```");

    expect(copyButton()).not.toBeNull();
  });

  it("클립보드 쓰기가 실패하면 복사됐다고 말하지 않는다", async () => {
    writeText.mockRejectedValueOnce(new Error("denied"));
    render("```\nnope\n```");

    await act(async () => copyButton()!.click());

    expect(host.querySelector('button[aria-label="복사됨"]')).toBeNull();
    expect(copyButton()).not.toBeNull();
  });

  it("인라인 코드에는 버튼을 달지 않는다", () => {
    render("이건 `inline` 코드다");

    expect(host.querySelector("button")).toBeNull();
  });
});

describe("메시지 전체가 HTML일 때", () => {
  const TABLE = "<table><tr><td>셀</td></tr></table>";

  it("이스케이프된 텍스트가 아니라 샌드박스 iframe으로 렌더한다", () => {
    render(TABLE);

    const frame = host.querySelector("iframe");
    expect(frame?.getAttribute("srcdoc")).toBe(TABLE);
    // 빈 sandbox — 스크립트·동일 출처를 절대 열어주지 않는다.
    expect(frame?.getAttribute("sandbox")).toBe("");
    expect(host.textContent).not.toContain("<table>");
  });

  it("HTML을 설명하는 문장은 그대로 마크다운이다", () => {
    render("`<table>` 태그는 뭔가요?");

    expect(host.querySelector("iframe")).toBeNull();
    expect(host.textContent).toContain("<table>");
  });
});

describe("스트리밍 중인 버블", () => {
  it("stable=false면 HTML로 판정하지 않는다 — 높이가 튀지 않게", () => {
    const table = "<table><tr><td>셀</td></tr></table>";
    act(() => root.render(<Markdown text={table} stable={false} />));

    expect(host.querySelector("iframe")).toBeNull();
  });
});
