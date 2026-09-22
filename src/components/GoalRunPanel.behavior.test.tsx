// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  stop: vi.fn(),
  create: vi.fn(),
}));

vi.mock("../lib/ipc", () => ({
  goalRunList: mocks.list,
  goalRunStop: mocks.stop,
  goalRunCreate: mocks.create,
}));

import { GoalRunPanel } from "./GoalRunPanel";

let container: HTMLDivElement;
let root: Root;

const view = (over: Record<string, unknown> = {}) => ({
  run: {
    id: 1,
    repo: "/tmp/repo",
    agent: "claude",
    instruction: "목표",
    goal_contract: {
      schema_version: 1,
      objective: "빌드를 통과시킨다",
      acceptance: [],
      stop_conditions: [],
      must_preserve: [],
      protected_paths: [],
      non_goals: [],
    },
    budget: {
      max_attempts: 3,
      max_tokens: 0,
      max_cost_usd: 5,
      max_wall_secs: 3600,
    },
    status: "running",
    created_at: 0,
    ended_at: null,
    end_reason: null,
    ...(over.run as object),
  },
  spent: { attempts: 1, tokens: 500, cost_usd: 0.25, elapsed_secs: 60, ...(over.spent as object) },
  attempt_task_ids: [11],
});

const mount = async () => {
  await act(async () => {
    root.render(<GoalRunPanel repo="/tmp/repo" />);
  });
};

const testId = (id: string) => container.querySelector(`[data-testid="${id}"]`);
const buttonText = (text: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes(text));

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  mocks.list.mockResolvedValue([view()]);
  mocks.stop.mockResolvedValue(undefined);
  mocks.create.mockResolvedValue(1);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.clearAllMocks();
});

describe("GoalRunPanel", () => {
  it("무제한(0) 예산에는 게이지 바를 그리지 않는다", async () => {
    await mount();
    // max_tokens = 0 → 분모가 없으므로 비율을 그릴 수 없다.
    expect(testId("budget-tokens")).not.toBeNull();
    expect(testId("budget-tokens-bar")).toBeNull();
    expect(testId("budget-tokens")?.textContent).toContain("무제한");
    // max_attempts = 3 → 바가 있다.
    expect(testId("budget-attempts-bar")).not.toBeNull();
  });

  it("벤더가 비용을 보고하지 않으면 0이 아니라 —로 그린다", async () => {
    // 토큰은 썼는데 비용이 0 = codex처럼 비용을 주지 않는 벤더다.
    mocks.list.mockResolvedValue([view({ spent: { attempts: 1, tokens: 500, cost_usd: 0, elapsed_secs: 60 } })]);
    await mount();
    const cost = testId("budget-cost");
    expect(cost?.textContent).toContain("—");
    expect(cost?.textContent).not.toContain("$0.00");
  });

  it("실제로 0달러를 쓴 경우(토큰도 0)는 미측정으로 감추지 않는다", async () => {
    mocks.list.mockResolvedValue([view({ spent: { attempts: 0, tokens: 0, cost_usd: 0, elapsed_secs: 1 } })]);
    await mount();
    expect(testId("budget-cost")?.textContent).toContain("$0.00");
  });

  it("종료된 Run에는 중단 버튼이 없고 종료 사유를 보여준다", async () => {
    mocks.list.mockResolvedValue([
      view({ run: { status: "exhausted", end_reason: "재진입 횟수 소진" } }),
    ]);
    await mount();
    expect(buttonText("중단")).toBeUndefined();
    expect(testId("run-1-reason")?.textContent).toContain("재진입 횟수 소진");
  });

  it("진행 중인 Run은 중단할 수 있다", async () => {
    await mount();
    await act(async () => {
      buttonText("중단")?.click();
    });
    expect(mocks.stop).toHaveBeenCalledWith(1);
  });

  it("예산이 전부 0이면 시작할 수 없다", async () => {
    await mount();
    const objective = container.querySelector<HTMLInputElement>('[aria-label="목표"]')!;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        "value",
      )!.set!;
      setter.call(objective, "무언가");
      objective.dispatchEvent(new Event("input", { bubbles: true }));
    });
    for (const label of ["재진입 상한", "토큰 상한", "비용 상한", "시간 상한"]) {
      const input = container.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)!;
      await act(async () => {
        const setter = Object.getOwnPropertyDescriptor(
          window.HTMLInputElement.prototype,
          "value",
        )!.set!;
        setter.call(input, "0");
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
    }
    expect(buttonText("Run 시작")?.hasAttribute("disabled")).toBe(true);
  });

  it("목표가 비어 있으면 시작할 수 없다", async () => {
    await mount();
    expect(buttonText("Run 시작")?.hasAttribute("disabled")).toBe(true);
  });
});
