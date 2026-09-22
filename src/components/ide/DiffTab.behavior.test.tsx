// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DiffHunk } from "../../lib/ipc";

const hunk: DiffHunk = {
  id: "h1",
  path: "src/a.ts",
  old_range: [1, 2],
  new_range: [1, 2],
  protected: false,
  committed: false,
  risk: "low",
  lines: [
    { kind: "context", text: "const a = 1;" },
    { kind: "add", text: "const b = 2;" },
  ],
};

const mocks = vi.hoisted(() => ({
  annotationsList: vi.fn(async () => []),
  diffHunks: vi.fn(),
  taskDiff: vi.fn(),
}));

vi.mock("../../lib/ipc", () => ({
  annotationSave: vi.fn(),
  annotationsList: mocks.annotationsList,
  annotationsResend: vi.fn(),
  diffHunks: mocks.diffHunks,
  partialApply: vi.fn(),
  partialRollback: vi.fn(),
  taskDiff: mocks.taskDiff,
}));

import { DiffSessionProvider } from "../DiffSessionContext";
import { DiffTab } from "./DiffTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** jsdom에는 ResizeObserver가 없다. 폭 전이는 관측 콜백을 손으로 발화시켜 검증한다. */
class ResizeObserverStub {
  static instances: ResizeObserverStub[] = [];
  constructor(private cb: ResizeObserverCallback) {
    ResizeObserverStub.instances.push(this);
  }
  observe() {}
  disconnect() {}
  emit(width: number) {
    this.cb([{ contentRect: { width } } as ResizeObserverEntry], this as never);
  }
}

const task = { host: "local", id: 9 };
const emit = async (width: number) => {
  await act(async () => {
    ResizeObserverStub.instances.forEach((observer) => observer.emit(width));
  });
};
const press = async (key: string) => {
  await act(async () => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
};
const buttonWith = (label: string) =>
  [...(container?.querySelectorAll("button") ?? [])].find((b) => b.textContent === label);

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const render = async (children: React.ReactNode) => {
  await act(async () => {
    root?.render(
      <DiffSessionProvider task={task} openDiff={() => {}}>
        {children}
      </DiffSessionProvider>,
    );
    await Promise.resolve();
  });
};

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", ResizeObserverStub);
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    cb(0);
    return 0;
  });
  localStorage.clear();
  ResizeObserverStub.instances = [];
  mocks.diffHunks.mockResolvedValue([hunk]);
  mocks.taskDiff.mockResolvedValue({
    files: [{ path: "src/a.ts", status: "M", patch: "@@ -1 +1 @@\n+const b = 2;" }],
    baseline: { kind: "pinned" as const },
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  container = null;
  root = null;
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

describe("DiffTab 폭 강제 unified (AC-6)", () => {
  it("좁아지면 분할을 잠그고 저장 모드를 건드리지 않는다", async () => {
    localStorage.setItem("praxis:diff-mode", "split");
    await render(<DiffTab path="src/a.ts" active />);
    expect(buttonWith("분할")?.getAttribute("aria-pressed")).toBe("true");

    await emit(600);
    expect(buttonWith("통합")?.getAttribute("aria-pressed")).toBe("true");
    expect(buttonWith("분할")?.hasAttribute("disabled")).toBe(true);

    // 강제 구간의 S는 저장값을 바꾸지 않는다 — 다른 탭과 다음 세션의 기본값이 걸려 있다.
    await press("s");
    await act(async () => buttonWith("분할")?.click());
    expect(localStorage.getItem("praxis:diff-mode")).toBe("split");
    expect(buttonWith("통합")?.getAttribute("aria-pressed")).toBe("true");

    await emit(900);
    expect(buttonWith("분할")?.getAttribute("aria-pressed")).toBe("true");
  });
});

describe("DiffTab 사라진 파일 (AC-9)", () => {
  it("스냅샷에서 빠져도 탭을 닫지 않고 안내와 닫기 버튼을 보인다", async () => {
    const onClose = vi.fn();
    await render(<DiffTab path="src/gone.ts" active onClose={onClose} />);

    expect(container?.textContent).toContain("이 파일은 더 이상 바뀌지 않았습니다");
    await act(async () => buttonWith("탭 닫기")?.click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("DiffTab 단축키 소유자 (AC-11)", () => {
  it("탭이 둘 마운트돼도 J는 활성 탭에서만 동작한다", async () => {
    const jump = vi.fn();
    Element.prototype.scrollIntoView = jump;
    await render(
      <>
        <DiffTab path="src/a.ts" active />
        <DiffTab path="src/a.ts" />
      </>,
    );
    expect(container?.querySelectorAll("[data-diff-tab]")).toHaveLength(2);

    await press("j");
    expect(jump).toHaveBeenCalledTimes(1);
  });
});

describe("DiffTab 세션 값 (DR-5)", () => {
  it("범위·기준점·↻·부분 적용 실행을 툴바에 두지 않는다", async () => {
    await render(<DiffTab path="src/a.ts" active />);

    expect(buttonWith("세션 전체")).toBeUndefined();
    expect(buttonWith("미커밋")).toBeUndefined();
    expect(container?.querySelector("[aria-label='Diff 새로고침']")).toBeNull();
    expect(container?.textContent).not.toContain("선택 적용");
  });
});

describe("DiffTab 단축키 (AC-5)", () => {
  it("U·S가 모드를 바꾸고 저장값에 남는다", async () => {
    await render(<DiffTab path="src/a.ts" active />);

    await press("s");
    expect(localStorage.getItem("praxis:diff-mode")).toBe("split");
    await press("u");
    expect(localStorage.getItem("praxis:diff-mode")).toBe("unified");
  });

  it("수식키 조합은 앱 전역 단축키에 넘긴다", async () => {
    await render(<DiffTab path="src/a.ts" active />);

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "s", metaKey: true }));
    });
    expect(localStorage.getItem("praxis:diff-mode")).toBeNull();
  });

  it("V는 이 탭의 파일을 확인함으로 표시하고 작업별로 남는다", async () => {
    await render(<DiffTab path="src/a.ts" active />);

    await press("v");
    expect(localStorage.getItem("praxis:diff-viewed:9")).toContain("src/a.ts");
    await press("v");
    expect(localStorage.getItem("praxis:diff-viewed:9")).not.toContain("src/a.ts");
  });
});
