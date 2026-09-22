// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CodeWikiPanel } from "./CodeWikiPanel";
import { useCodeWikiPanel } from "./useCodeWikiPanel";
import type { CodeWikiActions } from "./useCodeGraphPanel";
import type { CodeWikiStatus } from "../../lib/code-wiki-ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const STATUS: CodeWikiStatus = {
  graphState: "ready",
  indexPath: "docs/codebase/index.md",
  indexState: "ready",
  modules: [{ sourcePath: "src/main.rs", pagePath: "docs/codebase/modules/src/main.rs.md", state: "ready" }],
  detail: null,
};

let container: HTMLDivElement;
let root: Root;

function render(node: React.ReactNode): void {
  act(() => root.render(node));
}

function click(label: string): void {
  act(() => (container.querySelector(`[aria-label="${label}"]`) as HTMLButtonElement).click());
}

function Harness({ actions, scope, sourcePath = "src/main.rs", dirty = false }: {
  actions: CodeWikiActions;
  scope: number;
  sourcePath?: string;
  dirty?: boolean;
}) {
  const wiki = useCodeWikiPanel({ actions, scope, sourcePath, dirty });
  return (
    <>
      <button aria-label="Wiki 열기" onClick={wiki.show}>열기</button>
      <button aria-label="전체 생성" onClick={() => void wiki.generate(null)}>전체 생성</button>
      <output>{wiki.status?.indexPath}</output>
      {wiki.error && <p role="alert">{wiki.error}</p>}
    </>
  );
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

describe("CodeWikiPanel", () => {
  it("오래된 그래프에서는 갱신을 잠그고 안내한다", () => {
    render(
      <CodeWikiPanel
        status={{ ...STATUS, graphState: "stale" }} error={null} busy={false} sourcePath="src/main.rs" dirty={false}
        onGenerate={vi.fn()} onOpenPath={vi.fn()} onReload={vi.fn()} onClose={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("코드 그래프를 새로 고친 뒤 Wiki를 갱신하세요.");
    expect((container.querySelector("button") as HTMLButtonElement).disabled).toBe(false);
    expect((container.querySelectorAll("button")[1] as HTMLButtonElement).disabled).toBe(true);
  });

  it("저장하지 않은 Wiki 버퍼와 충돌을 사용자에게 남긴다", () => {
    render(
      <CodeWikiPanel
        status={{ ...STATUS, indexState: "conflict" }} error={null} busy={false} sourcePath="src/main.rs" dirty
        onGenerate={vi.fn()} onOpenPath={vi.fn()} onReload={vi.fn()} onClose={vi.fn()}
      />,
    );

    expect(container.textContent).toContain("저장하지 않은 소스 또는 Wiki 문서");
    expect(container.querySelectorAll('[role="alert"]')).toHaveLength(1);
    expect((container.querySelectorAll("button")[1] as HTMLButtonElement).disabled).toBe(true);
  });

  it("선택한 Rust 파일의 문서 경로와 갱신 경로를 사용한다", () => {
    const onGenerate = vi.fn();
    const onOpenPath = vi.fn();
    render(
      <CodeWikiPanel
        status={STATUS} error={null} busy={false} sourcePath="src/main.rs" dirty={false}
        onGenerate={onGenerate} onOpenPath={onOpenPath} onReload={vi.fn()} onClose={vi.fn()}
      />,
    );

    act(() => (container.querySelectorAll("button")[2] as HTMLButtonElement).click());
    act(() => (container.querySelectorAll("button")[4] as HTMLButtonElement).click());
    expect(onGenerate).toHaveBeenCalledWith("src/main.rs");
    expect(onOpenPath).toHaveBeenCalledWith("docs/codebase/modules/src/main.rs.md");
  });

  it("없는 목차와 현재 문서는 열지 않고 stale·conflict 문서는 열 수 있다", () => {
    render(
      <CodeWikiPanel
        status={{ ...STATUS, indexState: "missing", modules: [{ ...STATUS.modules[0], state: "missing" }] }} error={null} busy={false} sourcePath="src/main.rs" dirty={false}
        onGenerate={vi.fn()} onOpenPath={vi.fn()} onReload={vi.fn()} onClose={vi.fn()}
      />,
    );

    expect((container.querySelectorAll("button")[3] as HTMLButtonElement).disabled).toBe(true);
    expect((container.querySelectorAll("button")[4] as HTMLButtonElement).disabled).toBe(true);
    render(
      <CodeWikiPanel
        status={{ ...STATUS, indexState: "conflict", modules: [{ ...STATUS.modules[0], state: "stale" }] }} error={null} busy={false} sourcePath="src/main.rs" dirty={false}
        onGenerate={vi.fn()} onOpenPath={vi.fn()} onReload={vi.fn()} onClose={vi.fn()}
      />,
    );
    expect((container.querySelectorAll("button")[3] as HTMLButtonElement).disabled).toBe(false);
    expect((container.querySelectorAll("button")[4] as HTMLButtonElement).disabled).toBe(false);
  });
});

describe("useCodeWikiPanel", () => {
  it("중복 생성은 같은 범위에서 하나만 실행한다", async () => {
    const generate = vi.fn(() => new Promise<CodeWikiStatus>(() => undefined));
    const actions: CodeWikiActions = { status: vi.fn(async () => STATUS), generate, openPath: vi.fn() };
    render(<Harness actions={actions} scope={1} />);

    click("Wiki 열기");
    await act(async () => undefined);
    click("전체 생성");
    click("전체 생성");
    expect(generate).toHaveBeenCalledTimes(1);
  });

  it("실패를 alert로 보이고 늦은 이전 범위 응답은 무시한다", async () => {
    let resolveOld: ((value: CodeWikiStatus) => void) | undefined;
    const oldActions: CodeWikiActions = {
      status: vi.fn(() => new Promise<CodeWikiStatus>((resolve) => { resolveOld = resolve; })),
      generate: vi.fn(),
      openPath: vi.fn(),
    };
    const newActions: CodeWikiActions = {
      status: vi.fn(async () => STATUS),
      generate: vi.fn(async (): Promise<CodeWikiStatus> => { throw new Error("생성 실패"); }),
      openPath: vi.fn(),
    };
    render(<Harness actions={oldActions} scope={1} />);
    click("Wiki 열기");
    render(<Harness actions={newActions} scope={2} sourcePath="src/next.rs" />);
    click("Wiki 열기");
    await act(async () => undefined);
    await act(async () => resolveOld?.({ ...STATUS, indexPath: "old/index.md" }));
    expect(container.textContent).toContain("docs/codebase/index.md");
    expect(container.textContent).not.toContain("old/index.md");

    click("전체 생성");
    await act(async () => undefined);
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("생성 실패");
  });
});
