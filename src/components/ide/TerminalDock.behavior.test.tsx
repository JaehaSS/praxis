import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

// 실제 xterm은 로드만으로 canvas/WebGL을 건드린다 — 도크의 껍데기(높이·헤더·안내)만 보므로
// 셸은 자리 표시자로 바꾼다.
vi.mock("./ShellTerminal", () => ({
  ShellTerminal: ({ taskId }: { taskId: number }) => <div data-shell={taskId} />,
}));

const { TerminalDock } = await import("./TerminalDock");

const render = (props: Partial<Parameters<typeof TerminalDock>[0]> = {}): string =>
  renderToStaticMarkup(
    <TerminalDock taskId={7} available label="praxis-wt-7" onClose={() => {}} {...props} />,
  );

describe("하단 터미널 도크", () => {
  it("저장된 높이가 없으면 기본 높이로 연다", () => {
    expect(render()).toContain("height:260px");
  });

  it("어느 워크트리의 셸인지 헤더에 밝힌다", () => {
    expect(render()).toContain("praxis-wt-7");
  });

  it("높이 조절 손잡이를 키보드로도 잡을 수 있다", () => {
    const html = render();

    expect(html).toContain('aria-label="터미널 높이 조절"');
    expect(html).toContain('aria-orientation="horizontal"');
    expect(html).toContain('tabindex="0"');
  });

  it("로컬 작업이면 그 작업의 셸을 띄운다", () => {
    expect(render({ taskId: 12 })).toContain('data-shell="12"');
  });

  it("원격 작업이면 셸 대신 이유를 알린다", () => {
    const html = render({ available: false });

    expect(html).toContain("로컬 작업에서만");
    expect(html).not.toContain("data-shell");
  });
});
