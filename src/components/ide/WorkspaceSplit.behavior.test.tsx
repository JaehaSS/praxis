// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspaceSplit } from "./WorkspaceSplit";
import { MIN_CODE_WIDTH, SPLIT_ENTER, SPLIT_EXIT, writeSplit } from "./workspace-split-width";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/**
 * jsdom에는 ResizeObserver가 없다. 관측 콜백을 손으로 발화시켜 폭 전이만 검증한다 —
 * 실제 픽셀 배치는 jsdom이 계산하지 않으므로 수동 확인의 몫이다(플랜 DR-4).
 */
class ResizeObserverStub {
  static latest: ResizeObserverStub | null = null;
  constructor(private cb: ResizeObserverCallback) {
    ResizeObserverStub.latest = this;
  }
  observe() {}
  disconnect() {}
  emit(width: number) {
    this.cb([{ contentRect: { width } } as ResizeObserverEntry], this as never);
  }
}

const closeCode = vi.fn();

/** 코드 열은 헤더가 소유하므로 테스트도 그 자리에서 준다 — 기본은 열린 상태를 본다. */
const render = async (root: Root | null, codeOpen = true) => {
  await act(async () => {
    root?.render(
      <WorkspaceSplit
        session={<div>세션 내용</div>}
        code={<div>코드 내용</div>}
        codeOpen={codeOpen}
        onCloseCode={closeCode}
      />,
    );
  });
};

const emit = async (width: number) => {
  await act(async () => {
    ResizeObserverStub.latest?.emit(width);
  });
};

describe("WorkspaceSplit 폭 전이", () => {
  let container: HTMLDivElement | null = null;
  let root: Root | null = null;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    closeCode.mockClear();
    localStorage.clear();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root?.unmount());
    container?.remove();
    container = null;
    root = null;
    ResizeObserverStub.latest = null;
    vi.unstubAllGlobals();
  });

  // ResizeObserver의 첫 콜백 전에도 모드가 정해져야 한다 — 아니면 좁은 창에서 2열이
  // 한 프레임 그려졌다가 접히는 것이 눈에 띈다.
  it("관측 전에는 창 폭으로 모드를 추정한다", async () => {
    window.innerWidth = 800; // 크롬 몫을 빼면 2열 최소 요구에 못 미친다
    await render(root);

    expect(container?.textContent).toContain("코드 내용");
  });

  it("관측 전 추정이 넉넉하면 2열로 시작한다", async () => {
    window.innerWidth = 1600;
    await render(root);

    expect(container?.textContent).toContain("코드 내용");
  });

  it("넉넉한 폭에서는 두 열을 함께 보여준다", async () => {
    await render(root);
    await emit(1200);

    expect(container?.textContent).toContain("세션 내용");
    expect(container?.textContent).toContain("코드 내용");
  });

  it("폭이 최소 요구 아래로 내려가면 탭으로 폴백한다", async () => {
    await render(root);
    await emit(1200);
    await emit(SPLIT_EXIT - 1);

    expect(container?.textContent).toContain("코드 내용");
    expect(container?.firstElementChild?.firstElementChild?.className).toContain("hidden");
  });

  // 히스테리시스의 핵심 — 폴백 후 EXIT를 조금 넘겨도 돌아오지 않는다.
  it("폴백 상태에서 두 임계 사이로 돌아와도 탭을 유지한다", async () => {
    await render(root);
    await emit(1200);
    await emit(SPLIT_EXIT - 1);
    await emit(SPLIT_ENTER - 1);

    expect(container?.textContent).toContain("코드 내용");
  });

  it("복귀 임계를 넘으면 2열로 되돌아온다", async () => {
    await render(root);
    await emit(1200);
    await emit(SPLIT_EXIT - 1);
    await emit(SPLIT_ENTER);

    expect(container?.textContent).toContain("세션 내용");
    expect(container?.textContent).toContain("코드 내용");
  });

  // 상태는 헤더가 갖는다 — 여기서 직접 접지 않고 닫아 달라고 알리는 것이 전부다.
  it("경계 더블클릭은 코드 열 닫기를 위로 알린다", async () => {
    await render(root);
    await emit(1200);

    const separator = container?.querySelector('[role="separator"]');
    await act(async () => {
      separator?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    });

    expect(closeCode).toHaveBeenCalledTimes(1);
  });
});

/**
 * 이 변경의 요지 — 세션이 기본이고 코드 열은 부를 때만 온다(ADR 0111). 폭이 넉넉하든
 * 모자라든 `codeOpen`이 거짓이면 코드는 자리를 받지 않는다.
 */
describe("WorkspaceSplit 코드 열 노출", () => {
  let container: HTMLDivElement | null = null;
  let root: Root | null = null;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    closeCode.mockClear();
    localStorage.clear();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root?.unmount());
    container?.remove();
    container = null;
    root = null;
    ResizeObserverStub.latest = null;
    vi.unstubAllGlobals();
  });

  it("닫혀 있으면 폭이 넉넉해도 코드 열을 세우지 않는다", async () => {
    await render(root, false);
    await emit(1600);

    expect(container?.textContent).toContain("세션 내용");
    expect(container?.textContent).not.toContain("코드 내용");
  });

  // 닫힌 동안에는 폭을 나눌 상대가 없다 — 경계가 남아 있으면 세션이 전폭을 못 쓴다.
  it("닫혀 있으면 너비 조절 경계도 두지 않는다", async () => {
    await render(root, false);
    await emit(1600);

    expect(container?.querySelector('[role="separator"]')).toBeNull();
  });

  it("탭 폴백에서도 닫혀 있으면 세션만 보여준다", async () => {
    await render(root, false);
    await emit(SPLIT_EXIT - 1);

    expect(container?.textContent).toContain("세션 내용");
  });

  it("탭 폴백에서 열면 그 자리를 코드가 받는다", async () => {
    await render(root, true);
    await emit(SPLIT_EXIT - 1);

    expect(container?.textContent).toContain("코드 내용");
  });
});

/**
 * 저장 폭은 드래그하던 해상도에서만 검증된 값이다 — 표시 시점의 가용 폭이 그보다 좁으면
 * 다시 가둬야 한다. 다만 저장값 자체(의도한 폭)는 덮어쓰지 않으므로, 가용 폭이 돌아오면
 * 깎였던 폭이 그대로 복원된다.
 */
describe("WorkspaceSplit 저장 폭 재클램프", () => {
  let container: HTMLDivElement | null = null;
  let root: Root | null = null;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    closeCode.mockClear();
    localStorage.clear();
    writeSplit(localStorage, { width: 900 });
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root?.unmount());
    container?.remove();
    container = null;
    root = null;
    ResizeObserverStub.latest = null;
    vi.unstubAllGlobals();
  });

  it("저장 폭이 가용 폭을 넘으면 코드 열 최소 폭만큼 물러난다", async () => {
    await render(root);
    await emit(1000);

    const sessionColumn = container?.querySelector('[role="separator"]')?.previousElementSibling as HTMLElement | null;
    expect(sessionColumn?.style.width).toBe(`${1000 - MIN_CODE_WIDTH}px`);
    expect(container?.textContent).toContain("코드 내용");
  });

  it("가용 폭이 돌아오면 의도한 폭이 복원된다 — 저장값은 깎이지 않는다", async () => {
    await render(root);
    await emit(1000);
    await emit(1600);

    const sessionColumn = container?.querySelector('[role="separator"]')?.previousElementSibling as HTMLElement | null;
    expect(sessionColumn?.style.width).toBe("900px");
  });
});

/**
 * AC-1 — diff 본문은 코드 열 콘텐츠 폭을 그대로 쓴다(안에 파일 목록을 끼지 않는다, ADR 0175).
 * 그래서 "본문이 읽을 만한가"는 코드 열에 남는 폭 하나로 판정된다. 노트북 상용 폭 둘에서
 * 그 값이 코드 열 최소 폭 아래로 내려가지 않는 것을 잠근다.
 */
describe("WorkspaceSplit 코드 열 콘텐츠 폭 (AC-1)", () => {
  let container: HTMLDivElement | null = null;
  let root: Root | null = null;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    closeCode.mockClear();
    localStorage.clear();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root?.unmount());
    container?.remove();
    container = null;
    root = null;
    ResizeObserverStub.latest = null;
    vi.unstubAllGlobals();
  });

  // jsdom은 배치를 계산하지 않는다 — 세션 열만 확정 폭을 갖고 코드 열이 나머지를 flex로
  // 받으므로, 남는 폭은 `가용 - 세션`으로 잰다(경계 4px은 코드 열 몫이 아니라 그 사이다).
  it.each([1280, 1440])("가용 폭 %i에서 코드 열이 최소 가독 폭 이상을 받는다", async (available) => {
    await render(root);
    await emit(available);

    const sessionColumn = container?.querySelector('[role="separator"]')
      ?.previousElementSibling as HTMLElement | null;
    const codeWidth = available - parseFloat(sessionColumn?.style.width ?? "0");

    expect(container?.textContent).toContain("코드 내용");
    expect(codeWidth).toBeGreaterThanOrEqual(MIN_CODE_WIDTH);
  });
});
