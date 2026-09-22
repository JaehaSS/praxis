// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stamp } from "../../lib/fmt";
import type { Task } from "../../lib/ipc";
import type { ToolCostReport } from "../../lib/ipc";
import {
  ToolCostSection,
  WorkContextPanel,
  parseDiffStat,
  type WorkContextDiff,
} from "./WorkContextPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/**
 * 툴 비용은 패널이 마운트되면서 IPC로 읽어 온다 — 정적 마크업 테스트에서는 effect가 돌지
 * 않아 무해했지만, 실제로 마운트해 클릭하는 테스트에서는 Tauri 없는 환경에서 터진다.
 * 부가 정보라 빈 리포트로 대체한다.
 */
vi.mock("../../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/ipc")>()),
  taskToolCost: vi.fn(async () => ({
    rows: [],
    total_chars: 0,
    peak_context_tokens: 0,
    unattributed_tokens: 0,
  })),
}));

const task: Task = {
  id: 42,
  host: "local",
  repo: "/workspace/praxis",
  branch: "feature/JH2-side-panel",
  base: "main",
  worktree_path: "/workspace/praxis/.praxis/worktrees/task-42",
  instruction: "현재 작업 정보를 간략하게 보여준다",
  state: "Running",
  created_at: 1,
  updated_at: 2,
  agent: "codex",
  mode: "conversation",
};

describe("parseDiffStat", () => {
  it("git diff --stat의 파일·추가·삭제 수를 추출한다", () => {
    expect(
      parseDiffStat(
        " src/App.tsx | 30 +++++++++++++++++++++---------\n 4 files changed, 27 insertions(+), 8 deletions(-)",
      ),
    ).toEqual({ files: 4, additions: 27, deletions: 8 });
  });

  it("변경이 없거나 일부 수치만 있는 요약도 0으로 정규화한다", () => {
    expect(parseDiffStat("")).toEqual({ files: 0, additions: 0, deletions: 0 });
    expect(parseDiffStat(" 1 file changed, 3 insertions(+)")).toEqual({
      files: 1,
      additions: 3,
      deletions: 0,
    });
  });
});

describe("WorkContextPanel", () => {
  const renderPanel = (diff: WorkContextDiff, runtime: "local" | "remote" = "local") =>
    renderToStaticMarkup(
      <WorkContextPanel task={task} runtime={runtime} diff={diff} onRefresh={() => {}} />,
    );

  it("선택 작업의 환경과 변경 요약을 한 섹션에 보여준다", () => {
    const html = renderPanel({
      state: "ready",
      value: "4 files changed, 27 insertions(+), 8 deletions(-)",
    });

    expect(html).toContain('aria-label="작업 환경"');
    expect(html).toContain("환경");
    expect(html).toContain("변경 사항");
    expect(html).toContain("+27");
    expect(html).toContain("-8");
    expect(html).toContain("4개 파일");
    expect(html).toContain("로컬");
    expect(html).toContain("feature/JH2-side-panel");
    expect(html).toContain("main");
    expect(html).toContain("비교 기준");
    expect(html).toContain("실행 중…");
    expect(html).toContain("현재 작업 정보를 간략하게 보여준다");
    expect(html).toContain('aria-label="변경 사항 새로고침"');
  });

  it("원격·로딩·오류 상태를 추측하지 않고 명시한다", () => {
    expect(renderPanel({ state: "loading" }, "remote")).toContain("변경 사항 확인 중");
    const errorHtml = renderPanel({ state: "error" }, "remote");

    expect(errorHtml).toContain("원격");
    expect(errorHtml).toContain("변경 사항을 확인할 수 없음");
  });

  const now = Math.floor(Date.now() / 1000);
  const renderTask = (overrides: Partial<Task>) =>
    renderToStaticMarkup(
      <WorkContextPanel
        task={{ ...task, ...overrides }}
        runtime="local"
        diff={{ state: "loading" }}
        onRefresh={() => {}}
      />,
    );

  it("세션 시작 시각과 마지막으로 멈춘 시각을 절대·상대 표기로 함께 남긴다", () => {
    const html = renderTask({
      state: "AwaitingReview",
      created_at: now - 7200,
      updated_at: now - 600,
    });

    expect(html).toContain("시작");
    expect(html).toContain(stamp(now - 7200));
    expect(html).toContain("2h 전");
    expect(html).toContain("마지막 종료");
    expect(html).toContain(stamp(now - 600));
    expect(html).toContain("10m 전");
  });

  it("실행 중인 작업의 updated_at은 종료가 아니라 실행 시작으로 읽는다", () => {
    const html = renderTask({ state: "Running", created_at: now - 7200, updated_at: now - 60 });

    expect(html).toContain("실행 시작");
    expect(html).not.toContain("마지막 종료");
  });

  it("한 번도 실행되지 않았으면 종료 시각을 지어내지 않는다", () => {
    const html = renderTask({ state: "Queued", created_at: now - 30, updated_at: now - 30 });

    expect(html).toContain("아직 실행 기록 없음");
    expect(html).not.toContain("마지막 종료");
    expect(html).not.toContain("실행 시작");
  });

  it("인터뷰 모호성 점수가 있으면 목표 옆에 배지를, 없으면 생략한다", () => {
    const withScore: Task = {
      ...task,
      ambiguity: { score: 0.35, goal: 0.9, constraints: 0.6, success: 0.7 },
    };
    const html = renderToStaticMarkup(
      <WorkContextPanel
        task={withScore}
        runtime="local"
        diff={{ state: "loading" }}
        onRefresh={() => {}}
      />,
    );
    expect(html).toContain("모호성 0.35");
    expect(html).toContain("bg-status-awaiting/15");
    // 차원 툴팁
    expect(html).toContain("제약 0.60");

    const without = renderPanel({ state: "loading" });
    expect(without).not.toContain("모호성");
  });
});

describe("WorkContextPanel 접기", () => {
  const markup = (onCollapse?: () => void) =>
    renderToStaticMarkup(
      <WorkContextPanel
        task={task}
        runtime="local"
        diff={{ state: "loading" }}
        onRefresh={() => {}}
        onCollapse={onCollapse}
      />,
    );

  it("접기를 받으면 환경 헤더에 소환면 닫기를 낸다", () => {
    expect(markup(() => {})).toContain('aria-label="작업정보 접기"');
  });

  it("접기를 받지 않으면 닫기 버튼이 없다 — 코드 열 탭의 고정 자리는 닫히지 않는다", () => {
    const html = markup();

    expect(html).not.toContain("작업정보 접기");
    // 헤더의 다른 손잡이는 그대로다 — 닫기가 없다고 헤더가 사라지지는 않는다.
    expect(html).toContain('aria-label="변경 사항 새로고침"');
  });

  describe("클릭", () => {
    let host: HTMLDivElement;
    let root: Root | null = null;
    const onCollapse = vi.fn();

    beforeEach(() => {
      host = document.createElement("div");
      document.body.appendChild(host);
      root = createRoot(host);
      onCollapse.mockClear();
    });

    afterEach(async () => {
      await act(async () => root?.unmount());
      host.remove();
      root = null;
    });

    it("닫기를 누르면 접기 콜백이 한 번 간다", async () => {
      await act(async () => {
        root?.render(
          <WorkContextPanel
            task={task}
            runtime="local"
            diff={{ state: "loading" }}
            onRefresh={() => {}}
            onCollapse={onCollapse}
          />,
        );
      });

      const close = [...host.querySelectorAll("button")].find(
        (b) => b.getAttribute("aria-label") === "작업정보 접기",
      );
      expect(close).toBeTruthy();

      await act(async () => {
        close?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      });
      expect(onCollapse).toHaveBeenCalledTimes(1);
    });
  });
});

describe("ToolCostSection", () => {
  const report = (over: Partial<ToolCostReport> = {}): ToolCostReport => ({
    rows: [],
    total_chars: 0,
    peak_context_tokens: 0,
    unattributed_tokens: 0,
    ...over,
  });
  const row = (over: Partial<ToolCostReport["rows"][number]> = {}) => ({
    tool: "Read",
    calls: 3,
    chars: 12000,
    calls_unknown_size: 0,
    attributed_tokens: 4200,
    attributed_calls: 2,
    ...over,
  });

  it("기본은 총 토큰만 — 툴별 내역은 펼치기 전까지 감춘다", () => {
    const html = renderToStaticMarkup(
      <ToolCostSection
        report={report({
          rows: [row(), row({ tool: "Bash", attributed_tokens: 1300 })],
          peak_context_tokens: 91000,
          unattributed_tokens: 500,
        })}
      />,
    );
    expect(html).toContain("툴 컨텍스트 비용");
    // 4,200 + 1,300 + 미귀속 500
    expect(html).toContain("6,000토큰");
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain("Read");
    expect(html).not.toContain("12,000자");
    expect(html).not.toContain("최대 91,000");
    expect(html).not.toContain("미귀속 500토큰");
  });

  it("총량은 화면에 잘려 안 보이는 행까지 더한다", () => {
    const rows = ["a", "b", "c", "d", "e", "f"].map((t) => row({ tool: t, attributed_tokens: 1000 }));
    const html = renderToStaticMarkup(<ToolCostSection report={report({ rows })} />);
    expect(html).toContain("6,000토큰");
  });

  it("귀속된 토큰이 하나도 없으면 총량도 0이 아니라 대시로 둔다", () => {
    const html = renderToStaticMarkup(
      <ToolCostSection report={report({ rows: [row({ attributed_tokens: 0 })] })} />,
    );
    expect(html).toContain("—");
    expect(html).not.toContain("0토큰");
  });

  it("데이터가 없거나 행이 비면 섹션 자체를 렌더하지 않는다", () => {
    expect(renderToStaticMarkup(<ToolCostSection report={null} />)).toBe("");
    expect(renderToStaticMarkup(<ToolCostSection report={report()} />)).toBe("");
  });
});
