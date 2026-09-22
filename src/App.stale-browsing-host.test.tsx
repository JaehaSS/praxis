// @vitest-environment jsdom

/**
 * 저장된 탐색 호스트가 더는 등록되어 있지 않을 때 Wiki가 **보이는데 눌리지 않던** 회귀다.
 *
 * 사이드바는 폴백을 거친 `scopedHost`를 받아 항목을 그렸는데, 진입 게이트인 `openQuickLink`는
 * 폴백 전 `browsingHost`를 그대로 비교했다. 원격 프로필이 localStorage에 남았지만 연결되지
 * 않은 채 앱이 켜지면 두 값이 갈라져, 멀쩡해 보이는 항목을 눌러도 아무 일도 일어나지 않았다.
 * 실패해도 오류 하나 남지 않는 종류라 여기서 보는 것은 **클릭이 실제로 화면을 바꾼다**는 사실이다.
 */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// App은 마운트 시점에 저장된 선택을 한 번 읽는다 — import보다 먼저 심어야 한다.
vi.hoisted(() => {
  localStorage.setItem("praxis-browsing-host", "ssh-없어진-프로필");
  localStorage.setItem("praxis-orch-views-open", "1");
});

vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
vi.stubGlobal("matchMedia", (query: string) => ({
  matches: false, media: query, onchange: null,
  addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  dispatchEvent: () => false,
}));

const tauri = vi.hoisted(() => {
  const empty: Record<string, unknown> = {};
  return {
    invoke: vi.fn(async (cmd: string) => {
      if (!(cmd in empty)) {
        empty[cmd] = cmd === "notification_source_page"
          ? { source_id: "test", after: null, cursor: 0, watermark: 0, results: [] }
          : cmd.startsWith("notification_")
            ? { items: [], sources: [], enabled: false, delivery_error: null }
            : cmd === "usage_snapshot" ? { vendors: [] } : [];
      }
      return empty[cmd];
    }),
  };
});
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => undefined) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), openPath: vi.fn() }));
vi.mock("./lib/monaco", () => ({ langFromPath: () => "typescript" }));
vi.mock("@monaco-editor/react", async () => {
  const React = await import("react");
  return { default: () => React.createElement("div", { "data-testid": "monaco" }) };
});
vi.mock("@xterm/xterm", () => ({
  Terminal: class { cols = 80; rows = 24; options = {}; loadAddon() {} open() {} write() {} writeln() {} dispose() {} onData() { return { dispose() {} }; } onResize() { return { dispose() {} }; } },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} dispose() {} } }));
vi.mock("@xterm/addon-webgl", () => ({ WebglAddon: class { dispose() {} } }));
// 위키 본문은 이 테스트의 관심사가 아니다 — 화면이 **바뀌었다는 것**만 확인한다.
vi.mock("./components/WikiView", async () => {
  const React = await import("react");
  return { WikiView: () => React.createElement("div", { "data-testid": "wiki-view" }) };
});

import App from "./App";
import { WIKI_LOCAL_ONLY_REASON } from "./components/ide/Sidebar";

let container: HTMLDivElement;
let root: Root;

const wikiButton = (): HTMLButtonElement | undefined =>
  [...container.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Wiki");

beforeEach(async () => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => root.render(<App />));
  await act(async () => { await Promise.resolve(); });
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.clearAllMocks();
});

describe("등록되지 않은 탐색 호스트가 저장돼 있을 때", () => {
  it("Wiki 항목을 로컬로 되돌려 잠그지 않는다", () => {
    const button = wikiButton();

    expect(button, "폴백된 로컬 호스트에서는 Wiki 항목이 있어야 한다").toBeDefined();
    expect(button?.disabled).toBe(false);
  });

  it("그 Wiki 항목을 누르면 실제로 위키가 열린다", async () => {
    await act(async () => wikiButton()?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await act(async () => { await Promise.resolve(); });

    expect(container.querySelector('[data-testid="wiki-view"]')).not.toBeNull();
    expect(container.textContent).not.toContain(WIKI_LOCAL_ONLY_REASON);
  });
});
