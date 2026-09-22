// @vitest-environment jsdom

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Task, ToolCostReport } from "../../lib/ipc";
import { TaskGoalSection, ToolCostSection } from "./WorkContextPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const task: Task = {
  id: 42,
  host: "local",
  repo: "/workspace/praxis",
  branch: "feature/JH2-channel-disclosure",
  base: "main",
  worktree_path: "/workspace/praxis/.praxis/worktrees/task-42",
  instruction: "첫 줄\n둘째 줄\n셋째 줄\n넷째 줄 — 세 줄 클램프 밖으로 밀려나는 부분",
  state: "Running",
  created_at: 1,
  updated_at: 2,
  mode: "conversation",
};

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

async function render(node: ReactElement): Promise<void> {
  await act(async () => root?.render(node));
}

const toggle = async (): Promise<void> => {
  const button = container?.querySelector("button");
  await act(async () => {
    button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
};

const text = (): string => container?.textContent ?? "";

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

describe("ToolCostSection 펼치기", () => {
  it("펼치면 툴별 호출 수·원문 크기·귀속 토큰과 관측 최댓값이 드러난다", async () => {
    await render(
      <ToolCostSection
        report={report({ rows: [row({ calls_unknown_size: 2 })], peak_context_tokens: 91000 })}
      />,
    );

    expect(text()).not.toContain("Read");

    await toggle();

    expect(text()).toContain("Read");
    expect(text()).toContain("3회");
    expect(text()).toContain("12,000자");
    expect(text()).toContain("4,200토큰");
    expect(text()).toContain("(2건 미상)");
    expect(text()).toContain("최대 91,000");
    expect(container?.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");
  });

  it("귀속하지 못한 툴은 펼친 뒤에도 0이 아니라 대시로 남는다", async () => {
    await render(<ToolCostSection report={report({ rows: [row({ attributed_tokens: 0 })] })} />);
    await toggle();

    expect(text()).toContain("—");
    expect(text()).not.toContain("0토큰");
  });

  it("미귀속 증가분은 펼침 본문의 별도 행으로 분리한다", async () => {
    await render(
      <ToolCostSection report={report({ rows: [row()], unattributed_tokens: 8100 })} />,
    );
    await toggle();

    expect(text()).toContain("미귀속 8,100토큰");
  });

  it("펼쳐도 툴 목록은 상위 5개까지만 그린다", async () => {
    const rows = ["a", "b", "c", "d", "e", "f"].map((t) => row({ tool: t }));
    await render(<ToolCostSection report={report({ rows })} />);
    await toggle();

    const tools = [...(container?.querySelectorAll("dt") ?? [])].map((el) => el.textContent);
    expect(tools).toEqual(["a", "b", "c", "d", "e"]);
  });

  it("다시 누르면 총량 한 줄로 되돌아간다", async () => {
    await render(<ToolCostSection report={report({ rows: [row()] })} />);
    await toggle();
    await toggle();

    expect(text()).not.toContain("12,000자");
    expect(text()).toContain("4,200토큰");
  });
});

describe("TaskGoalSection 펼치기", () => {
  const goalParagraph = (): HTMLParagraphElement | null =>
    container?.querySelector("p") ?? null;

  it("기본은 세 줄 클램프, 펼치면 원문 줄바꿈까지 전부 보여준다", async () => {
    await render(<TaskGoalSection task={task} />);

    expect(goalParagraph()?.className).toContain("line-clamp-3");
    expect(goalParagraph()?.className).not.toContain("whitespace-pre-wrap");

    await toggle();

    expect(goalParagraph()?.className).toContain("whitespace-pre-wrap");
    expect(goalParagraph()?.className).not.toContain("line-clamp-3");
    // 지시문이 길어도 채널을 독차지하지 않는다.
    expect(goalParagraph()?.className).toContain("max-h-56");
    expect(text()).toContain("넷째 줄");
  });

  it("목표가 비어 있으면 펼칠 것이 없으므로 토글을 두지 않는다", async () => {
    await render(<TaskGoalSection task={{ ...task, instruction: "" }} />);

    expect(container?.querySelector("button")).toBeNull();
    expect(text()).toContain("등록된 작업 목표가 없습니다.");
  });
});
