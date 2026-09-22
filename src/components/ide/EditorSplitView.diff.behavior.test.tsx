// @vitest-environment jsdom

import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  annotationsList: vi.fn(async () => []),
  diffHunks: vi.fn(async () => []),
  taskDiff: vi.fn(),
}));

// Monaco 본체는 jsdom에서 뜨지 않는다 — 여기서 보는 것은 "어느 탭이 어느 본문을 받는가"뿐이라
// 에디터 자리는 경로를 단 표식 하나로 대신한다.
vi.mock("@monaco-editor/react", async () => {
  const React = await import("react");
  return {
    default: ({ path }: { path: string }) =>
      React.createElement("div", { "data-testid": "monaco", "data-path": path }),
  };
});
vi.mock("../../lib/monaco", () => ({ langFromPath: () => "typescript" }));
vi.mock("./Mermaid", () => ({ Mermaid: () => null }));
vi.mock("../../lib/designmode/editor-capture-target", () => ({
  registerEditorCaptureTarget: () => () => undefined,
}));
vi.mock("../../lib/ipc", async (original) => ({
  ...(await original<Record<string, unknown>>()),
  annotationSave: vi.fn(),
  annotationsList: mocks.annotationsList,
  annotationsResend: vi.fn(),
  diffHunks: mocks.diffHunks,
  partialApply: vi.fn(),
  partialRollback: vi.fn(),
  taskDiff: mocks.taskDiff,
}));

import { DiffSessionProvider } from "../DiffSessionContext";
import { EditorSplitView } from "./EditorSplitView";
import { TAB_DRAG_MIME } from "./editor-drag";
import type { OpenFile } from "./EditorPane";
import { diffTabKey, fileTabKey, type TabKey } from "../../lib/tab-key";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

class ResizeObserverStub {
  observe() {}
  disconnect() {}
}

const PATH = "src/a.ts";

const fileTab = (path: string): OpenFile => ({
  key: fileTabKey(path),
  path,
  kind: "text",
  content: "본문",
  baseContent: "본문",
  mtime: 0,
  dirty: false,
});

const diffTab = (path: string): OpenFile => ({
  key: diffTabKey(path),
  path,
  kind: "diff",
  content: "",
  baseContent: "",
  mtime: 0,
  dirty: false,
});

/**
 * `useWorkspaceFiles`의 계약만 흉내 낸 부모 — 같은 경로의 파일 탭과 diff 탭을 함께 들려 준다.
 * 이 조합이 리뷰 2회차 Critical의 자리다: 경로로 잡으면 뒤엣것이 앞엣것을 덮는다.
 */
function Harness({ initial }: { initial: OpenFile[] }) {
  const [files, setFiles] = useState<OpenFile[]>(initial);
  const [activeKey, setActiveKey] = useState<TabKey | null>(initial[0]?.key ?? null);
  return (
    <DiffSessionProvider task={{ host: "local", id: 3 }} openDiff={() => {}}>
      <EditorSplitView
        taskId={3}
        host="local"
        windowId="test"
        files={files}
        activeKey={activeKey}
        dark={false}
        shortcutsActive
        onSelect={setActiveKey}
        onOpenFile={async () => false}
        onClose={(key) => {
          setFiles((current) => current.filter((f) => f.key !== key));
          setActiveKey((current) => (current === key ? null : current));
        }}
        onChange={() => undefined}
        onSave={() => undefined}
        onReload={() => undefined}
        onOpenPath={() => undefined}
        onRevealPath={() => undefined}
      />
    </DiffSessionProvider>
  );
}

let container: HTMLDivElement;
let root: Root;

const render = async (initial: OpenFile[]) => {
  await act(async () => {
    root.render(<Harness initial={initial} />);
    await Promise.resolve();
  });
};

const panes = () => Array.from(container.querySelectorAll<HTMLElement>("[data-focused]"));
const tabKeysOf = (pane: HTMLElement) =>
  Array.from(pane.querySelectorAll<HTMLElement>("[data-tab-key]")).map(
    (el) => el.dataset.tabKey ?? "",
  );
const tabIn = (pane: HTMLElement, key: TabKey) =>
  pane.querySelector<HTMLElement>(`[data-tab-key="${key}"]`);

const click = async (el: Element | null | undefined) => {
  expect(el, "클릭할 요소를 찾지 못했다").toBeTruthy();
  await act(async () => {
    el?.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    el?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await Promise.resolve();
  });
};

/** jsdom에는 DataTransfer가 없다 — 값을 나르는 통과 사각형 하나로 대신한다. */
class FakeDataTransfer {
  private store = new Map<string, string>();
  dropEffect = "none";
  effectAllowed = "none";
  get types(): string[] {
    return Array.from(this.store.keys());
  }
  setData(type: string, value: string): void {
    this.store.set(type, value);
  }
  getData(type: string): string {
    return this.store.get(type) ?? "";
  }
}

const PANE = { width: 1000, height: 600 };

const dragEvent = (type: string, data: FakeDataTransfer, x = 0, y = 0): Event => {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y });
  Object.defineProperty(event, "dataTransfer", { value: data });
  return event;
};

let restoreRect: (() => void) | null = null;
let animationFrame = 0;
let animationFrames = new Map<number, FrameRequestCallback>();

const flushAnimationFrame = async () => {
  const callbacks = Array.from(animationFrames.values());
  animationFrames.clear();
  await act(async () => callbacks.forEach((callback) => callback(0)));
};

/** 모든 요소가 같은 사각형을 갖는다 — 가장자리 판정이 결정적이 된다. */
const stubLayout = () => {
  const original = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function rect(): DOMRect {
    return {
      left: 0,
      top: 0,
      width: PANE.width,
      height: PANE.height,
      right: PANE.width,
      bottom: PANE.height,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect;
  };
  restoreRect = () => {
    HTMLElement.prototype.getBoundingClientRect = original;
  };
};

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", ResizeObserverStub);
  animationFrame = 0;
  animationFrames = new Map();
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    const id = ++animationFrame;
    animationFrames.set(id, callback);
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => animationFrames.delete(id));
  mocks.taskDiff.mockResolvedValue({
    files: [{ path: PATH, status: "M", patch: "@@ -1 +1 @@\n+const b = 2;" }],
    baseline: { kind: "pinned" as const },
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  restoreRect?.();
  restoreRect = null;
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

/**
 * AC-4 — 같은 경로의 두 탭은 키가 다르므로 한 자리를 놓고 다투지 않는다. 배치가 경로로
 * 잡던 시절에는 뒤에 열린 쪽이 앞엣것을 덮어, 탭 하나가 조용히 사라졌다.
 */
describe("EditorSplitView 같은 경로의 두 탭 (AC-4)", () => {
  it("파일 탭과 diff 탭이 한 칸에 나란히 남는다", async () => {
    await render([fileTab(PATH), diffTab(PATH)]);

    expect(tabKeysOf(panes()[0])).toEqual([fileTabKey(PATH), diffTabKey(PATH)]);
  });

  it("고른 탭이 제 본문을 받는다 — 키로 잡으므로 서로를 덮지 않는다", async () => {
    await render([fileTab(PATH), diffTab(PATH)]);

    await click(tabIn(panes()[0], diffTabKey(PATH)));
    expect(container.querySelector(`[data-diff-tab="${PATH}"]`)).toBeTruthy();
    expect(container.querySelector("[data-testid='monaco']")).toBeNull();

    await click(tabIn(panes()[0], fileTabKey(PATH)));
    expect(container.querySelector("[data-testid='monaco']")?.getAttribute("data-path")).toBe(PATH);
    expect(container.querySelector(`[data-diff-tab="${PATH}"]`)).toBeNull();
  });

  it("diff 탭을 닫아도 파일 탭은 남는다", async () => {
    await render([fileTab(PATH), diffTab(PATH)]);

    const closeButton = tabIn(panes()[0], diffTabKey(PATH))?.querySelector("[aria-label='닫기']");
    await click(closeButton);

    expect(tabKeysOf(panes()[0])).toEqual([fileTabKey(PATH)]);
  });
});

/** diff 탭의 `content`는 구조상 늘 빈 문자열이다 — 본문은 세션 스냅샷이 그린다. */
describe("EditorSplitView diff 탭 우클릭 메뉴", () => {
  const openMenu = async (pane: HTMLElement, key: TabKey) => {
    await act(async () => {
      tabIn(pane, key)?.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 20, clientY: 20 }),
      );
    });
    return container.ownerDocument.querySelector('[role="menu"]');
  };
  const labels = (menu: Element | null) =>
    Array.from(menu?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? []).map(
      (el) => el.textContent,
    );

  it("내용 복사를 내주지 않는다 — 빈 문자열이 소리 없이 복사되던 자리다", async () => {
    await render([fileTab(PATH), diffTab(PATH)]);

    expect(labels(await openMenu(panes()[0], diffTabKey(PATH)))).not.toContain("내용 복사");
  });

  it("파일 탭에서는 그대로 있다", async () => {
    await render([fileTab(PATH), diffTab(PATH)]);

    expect(labels(await openMenu(panes()[0], fileTabKey(PATH)))).toContain("내용 복사");
  });
});

/**
 * AC-14 — 드래그 페이로드는 JSON을 거치므로 `TabKey` 브랜드가 소실된다. 접두까지 잃으면
 * 옮겨 놓은 diff 탭이 같은 이름의 파일 탭으로 되살아난다.
 */
describe("EditorSplitView diff 탭 드래그 (AC-14)", () => {
  it("다른 칸으로 옮겨도 diff 탭으로 남는다", async () => {
    await render([fileTab(PATH), diffTab(PATH)]);
    stubLayout();

    const data = new FakeDataTransfer();
    await act(async () => {
      tabIn(panes()[0], diffTabKey(PATH))?.dispatchEvent(dragEvent("dragstart", data));
    });
    await flushAnimationFrame();
    expect(data.getData(TAB_DRAG_MIME)).toContain(diffTabKey(PATH));

    const surface = panes()[0].querySelector<HTMLElement>("[data-drop-surface]");
    expect(surface, "드롭 면이 깔리지 않았다").toBeTruthy();
    await act(async () => {
      surface?.dispatchEvent(dragEvent("dragover", data, PANE.width - 10, PANE.height / 2));
    });
    await act(async () => {
      surface?.dispatchEvent(dragEvent("drop", data, PANE.width - 10, PANE.height / 2));
      await Promise.resolve();
    });

    expect(panes()).toHaveLength(2);
    expect(tabKeysOf(panes()[0])).toEqual([fileTabKey(PATH)]);
    expect(tabKeysOf(panes()[1])).toEqual([diffTabKey(PATH)]);
    // 새 칸이 그리는 것도 여전히 변경분이다 — 파일로 되살아나지 않았다.
    expect(panes()[1].querySelector(`[data-diff-tab="${PATH}"]`)).toBeTruthy();
  });
});
