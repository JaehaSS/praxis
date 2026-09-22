// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
// 패널 안의 다른 섹션이 마운트에서 이벤트를 구독한다 — Tauri 런타임이 없는 곳이라 껍데기로 받는다.
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => undefined) }));
// 설정 패널은 에디터 탭을 통해 monaco를 끌어온다. monaco 본체는 import만으로 브라우저 API를
// 만지므로 jsdom에서 로드되지 않는다 — 이 테스트가 보는 것은 설정 행 하나뿐이라 껍데기로 세운다.
vi.mock("../../lib/monaco", () => ({ langFromPath: () => "typescript" }));
vi.mock("@monaco-editor/react", async () => {
  const React = await import("react");
  return { default: () => React.createElement("div", { "data-testid": "monaco" }) };
});

// 부분 mock — 설정 패널은 마운트에 수십 개의 IPC를 부르고, 여기서 바꾸는 것은 라운드 상한 둘뿐이다.
const ipc = vi.hoisted(() => ({ get: vi.fn(), set: vi.fn() }));
vi.mock("../../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/ipc")>()),
  debateRoundCapGet: ipc.get,
  debateRoundCapSet: ipc.set,
}));

import { SettingsPanel } from "./SettingsPanel";

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const panel = (
  <SettingsPanel
    // 라운드 상한은 "작업 실행" 탭에 있다 — 기본 탭은 "모양새"라 지목하지 않으면 렌더되지 않는다.
    initialTab="run"
    fontSettings={null}
    onFontSettings={() => undefined}
    editorSettings={{ tree_font_size: 13, minimap: false, word_wrap: true, tab_size: 2 }}
    onEditorSettings={() => undefined}
    onUseWorktreeChange={() => undefined}
  />
);

const capInput = () => container?.querySelector<HTMLInputElement>('input[aria-label="토론 라운드 상한"]') ?? null;

const type = async (value: string) => {
  const input = capInput();
  if (!input) throw new Error("라운드 상한 입력이 없다");
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    setter?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  // React의 onBlur는 위임된 focusout이다 — 버블하지 않는 blur는 핸들러에 닿지 않는다.
  await act(async () => input.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
};

beforeEach(async () => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  ipc.get.mockReset().mockResolvedValue(3);
  ipc.set.mockReset().mockResolvedValue(undefined);
  await act(async () => root?.render(panel));
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.restoreAllMocks();
});

describe("SettingsPanel 토론 라운드 상한", () => {
  it("저장된 값을 보여 주고, 범위 안의 값은 저장한 뒤 저장됨을 표시한다", async () => {
    expect(capInput()?.value).toBe("3");
    await type("4");
    expect(ipc.set).toHaveBeenCalledWith(4);
    expect(container?.textContent).toContain("저장됨");
  });

  it("범위 밖 입력은 저장을 부르지 않고 이전 값으로 되돌린다 — 클램프해 삼키지 않는다", async () => {
    await type("7");
    expect(ipc.set).not.toHaveBeenCalled();
    expect(capInput()?.value).toBe("3");
    expect(container?.textContent).toContain("2~5만 저장합니다");
    await type("1");
    expect(ipc.set).not.toHaveBeenCalled();
    await type("");
    expect(ipc.set).not.toHaveBeenCalled();
    expect(capInput()?.value).toBe("3");
  });
});
