// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CodeGraphControl, CodeGraphPanel } from "./CodeGraphPanel";
import type { CodeGraphImpact, CodeGraphStatus } from "../../lib/ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const absent: CodeGraphStatus = {
  activeState: "absent",
  activeRunId: null,
  indexedAt: null,
  files: 0,
  symbols: 0,
  edges: 0,
  buildState: "idle",
  buildRunId: null,
  detail: null,
  incomplete: null,
};

const impact: CodeGraphImpact = {
  runId: 3,
  indexedAt: 100,
  freshness: "ready",
  truncated: false,
  edgesUnavailable: null,
  items: [
    {
      id: 1,
      name: "direct",
      container: null,
      relPath: "src/direct.rs",
      line: 4,
      character: 2,
      depth: 1,
    },
    {
      id: 2,
      name: "indirect",
      container: "Outer",
      relPath: "src/indirect.rs",
      line: 8,
      character: 1,
      depth: 2,
    },
  ],
};

let container: HTMLDivElement;
let root: Root;

function render(node: React.ReactNode): void {
  act(() => root.render(node));
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("CodeGraphControl", () => {
  it("인덱스가 없으면 만들기만 열고 영향 범위는 잠근다", () => {
    render(
      <CodeGraphControl
        status={absent}
        dirty={false}
        busy={false}
        onIndex={vi.fn()}
        onCancel={vi.fn()}
        onImpact={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("코드 그래프: 없음");
    expect(container.textContent).toContain("그래프 만들기");
    expect(
      (container.querySelector('[aria-label="영향 범위 보기"]') as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it("언어 분석 대기 중에는 취소를 제공하고 준비라고 부르지 않는다", () => {
    render(
      <CodeGraphControl
        status={{ ...absent, buildState: "waiting_semantic", buildRunId: 4 }}
        dirty={false}
        busy
        onIndex={vi.fn()}
        onCancel={vi.fn()}
        onImpact={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("언어 분석 대기");
    expect(container.querySelector('[aria-label="코드 그래프 인덱싱 취소"]')).not.toBeNull();
    expect(container.textContent).not.toContain("준비됨");
  });

  it("최신 그래프여도 참조를 못 만든 언어를 신선도 옆에 따로 알린다", () => {
    render(
      <CodeGraphControl
        status={{
          ...absent,
          activeState: "ready",
          activeRunId: 3,
          incomplete: {
            filesSkipped: 0,
            filesWithoutEdges: 12,
            languagesWithoutEdges: ["python"],
            detail: "python: pyright 준비 신호 없음",
          },
        }}
        dirty={false}
        busy={false}
        onIndex={vi.fn()}
        onCancel={vi.fn()}
        onImpact={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("코드 그래프: 준비됨");
    expect(container.textContent).toContain("python 참조 분석 없음 (12개 파일)");
  });

  it("활성 그래프는 dirty 편집 즉시 오래됨으로 표시하고 영향 범위 조회를 잠근다", () => {
    render(
      <CodeGraphControl
        status={{ ...absent, activeState: "ready", activeRunId: 3, indexedAt: 100 }}
        dirty
        busy={false}
        onIndex={vi.fn()}
        onCancel={vi.fn()}
        onImpact={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("코드 그래프: 오래됨");
    expect(container.textContent).toContain("새로 고침");
    expect(
      (container.querySelector('[aria-label="영향 범위 보기"]') as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it("직전 활성 그래프를 유지하면서 새 빌드 실패를 숨기지 않는다", () => {
    render(
      <CodeGraphControl
        status={{ ...absent, activeState: "ready", activeRunId: 3, buildState: "degraded" }}
        dirty={false}
        busy={false}
        onIndex={vi.fn()}
        onCancel={vi.fn()}
        onImpact={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("준비됨 · 새 빌드 일부");
  });

  it("실패 detail을 툴팁에만 숨기지 않고 제어에 표시한다", () => {
    render(
      <CodeGraphControl
        status={{ ...absent, buildState: "failed", buildRunId: 4, detail: "rust-analyzer를 찾을 수 없습니다" }}
        dirty={false}
        busy={false}
        onIndex={vi.fn()}
        onCancel={vi.fn()}
        onImpact={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("코드 그래프: 실패");
    expect(container.querySelector('[role="status"]')?.textContent).toContain("rust-analyzer를 찾을 수 없습니다");
  });
});

describe("CodeGraphPanel", () => {
  it("직접·간접 영향을 나누고 클릭한 위치를 연다", () => {
    const onOpen = vi.fn();
    render(
      <CodeGraphPanel
        impact={{ ...impact, freshness: "stale" }}
        error={null}
        onOpen={onOpen}
        onClose={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("직접 영향 · 1");
    expect(container.textContent).toContain("간접 영향 · 1");
    expect(container.textContent).toContain("오래된 인덱스");
    expect(container.textContent).toContain("src/direct.rs:5");
    act(() => {
      (container.querySelector('[data-impact-id="1"]') as HTMLButtonElement).click();
    });
    expect(onOpen).toHaveBeenCalledWith(impact.items[0]);
  });

  it("엣지를 못 만든 파일의 빈 결과를 영향 없음으로 부르지 않는다", () => {
    render(
      <CodeGraphPanel
        impact={{
          ...impact,
          items: [],
          edgesUnavailable: "pyright: 준비 신호 없음",
        }}
        error={null}
        onOpen={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("참조를 분석하지 못했습니다");
    expect(container.textContent).toContain("pyright: 준비 신호 없음");
    expect(container.textContent).not.toContain("참조하는 인덱싱된 심볼이 없습니다");
  });
});
