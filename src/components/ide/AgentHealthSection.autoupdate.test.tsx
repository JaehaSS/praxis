// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AutoUpdateReport, HubUpdate } from "../../lib/ipc";

const mocks = vi.hoisted(() => ({
  agentHealth: vi.fn(async () => ({ vendors: [], checked_at: 0 })),
  agentAuthReconcile: vi.fn(async () => 0),
  autoUpdateGet: vi.fn(async () => true),
  autoUpdateSet: vi.fn(async (enabled: boolean) => enabled),
  autoUpdateLast: vi.fn(async (): Promise<AutoUpdateReport> => empty()),
  antigravityHubUpdate: vi.fn(async (): Promise<HubUpdate> => quietHub()),
  unlisten: vi.fn(),
  listen: vi.fn(),
}));

function empty(): AutoUpdateReport {
  return { outcomes: [], skipped: null, finished_at: 0 };
}

function quietHub(): HubUpdate {
  return { installed: "2.11.0", downloaded: "2.11.0", restart_required: false };
}

vi.mock("../../lib/ipc", () => ({
  agentHealth: mocks.agentHealth,
  agentAuthReconcile: mocks.agentAuthReconcile,
  autoUpdateGet: mocks.autoUpdateGet,
  autoUpdateSet: mocks.autoUpdateSet,
  autoUpdateLast: mocks.autoUpdateLast,
  antigravityHubUpdate: mocks.antigravityHubUpdate,
  AUTO_UPDATE_EVENT: "autoupdate://done",
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mocks.listen,
}));

// 액션 터미널은 xterm을 끌고 온다 — 이 테스트의 관심사가 아니다.
vi.mock("./ActionTerminal", () => ({ ActionTerminal: () => null }));

import { AgentHealthSection } from "./AgentHealthSection";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

async function mount() {
  await act(async () => {
    root.render(<AgentHealthSection />);
  });
}

const toggle = () => container.querySelector('[role="switch"]') as HTMLButtonElement;

beforeEach(() => {
  vi.clearAllMocks();
  mocks.agentHealth.mockResolvedValue({ vendors: [], checked_at: 0 });
  mocks.autoUpdateGet.mockResolvedValue(true);
  mocks.autoUpdateSet.mockImplementation(async (enabled: boolean) => enabled);
  mocks.autoUpdateLast.mockResolvedValue(empty());
  mocks.antigravityHubUpdate.mockResolvedValue(quietHub());
  mocks.listen.mockResolvedValue(mocks.unlisten);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("AgentHealthSection — 자동 업데이트", () => {
  it("건너뛴 이유를 화면에 내놓는다", async () => {
    // 백엔드가 만든 사유가 어디에도 안 나오면 조용한 실패와 구분되지 않는다.
    mocks.autoUpdateLast.mockResolvedValue({
      outcomes: [],
      skipped: "작업 2개가 돌고 있어 건너뜁니다 — 업데이트는 바이너리를 교체합니다",
      finished_at: 100,
    });
    await mount();
    expect(container.textContent).toContain("작업 2개가 돌고 있어 건너뜁니다");
  });

  it("무엇이 갱신됐는지 보여준다", async () => {
    mocks.autoUpdateLast.mockResolvedValue({
      outcomes: [
        {
          vendor: "claude",
          label: "Claude Code",
          from: "2.1.247",
          to: "2.1.250",
          ok: true,
          error: null,
        },
      ],
      skipped: null,
      finished_at: 100,
    });
    await mount();
    expect(container.textContent).toContain("Claude Code 2.1.247 → 2.1.250");
  });

  it("낡은 리포트가 새 리포트를 덮지 않는다", async () => {
    // 마운트 조회와 이벤트 수신은 서로 async라 도착 순서가 보장되지 않는다.
    let deliver: ((report: AutoUpdateReport) => void) | null = null;
    mocks.listen.mockImplementation(
      async (_event: string, handler: (e: { payload: AutoUpdateReport }) => void) => {
        deliver = (report) => handler({ payload: report });
        return mocks.unlisten;
      },
    );
    mocks.autoUpdateLast.mockResolvedValue({
      outcomes: [],
      skipped: "오래된 결과",
      finished_at: 10,
    });
    await mount();
    await act(async () => {
      deliver?.({ outcomes: [], skipped: "새 결과", finished_at: 99 });
    });
    expect(container.textContent).toContain("새 결과");

    await act(async () => {
      deliver?.({ outcomes: [], skipped: "뒤늦게 도착한 옛 결과", finished_at: 5 });
    });
    expect(container.textContent).toContain("새 결과");
    expect(container.textContent).not.toContain("뒤늦게 도착한 옛 결과");
  });

  it("저장에 실패하면 토글을 되돌리고 이유를 말한다", async () => {
    // 저장이 실패했는데 켜진 것처럼 보이면 다음 시작에 왜 안 도는지 알 수 없다.
    mocks.autoUpdateSet.mockRejectedValue(new Error("DB가 없습니다"));
    await mount();
    expect(toggle().getAttribute("aria-checked")).toBe("true");

    await act(async () => {
      toggle().click();
    });
    expect(toggle().getAttribute("aria-checked")).toBe("true");
    expect(container.textContent).toContain("DB가 없습니다");
  });

  it("설정을 읽지 못하면 토글을 잠근다", async () => {
    // 기본값을 켜짐으로 보여주면, 꺼둔 사용자가 누를 때 끄려던 의도가 켜기로 뒤집힌다.
    mocks.autoUpdateGet.mockRejectedValue(new Error("읽기 실패"));
    await mount();
    expect(toggle().disabled).toBe(true);
    expect(container.textContent).toContain("설정을 읽지 못했습니다");
  });

  it("Hub가 재시작을 기다리면 알리되 버튼은 주지 않는다", async () => {
    // 설치는 앱 종료를 요구하고, 남의 앱을 대신 끄지는 않는다.
    mocks.antigravityHubUpdate.mockResolvedValue({
      installed: "2.9.1",
      downloaded: "2.11.0",
      restart_required: true,
    });
    await mount();
    expect(container.textContent).toContain("Antigravity 2.9.1 → 2.11.0");
    expect(container.textContent).toContain("재시작하면 적용");
  });

  it("최신이면 Hub 이야기를 꺼내지 않는다", async () => {
    await mount();
    expect(container.textContent).not.toContain("Antigravity");
  });

  it("언마운트하면 이벤트 구독을 푼다", async () => {
    await mount();
    await act(async () => root.unmount());
    expect(mocks.unlisten).toHaveBeenCalled();
    // afterEach의 두 번째 unmount가 터지지 않도록 새 root를 끼워둔다.
    root = createRoot(document.createElement("div"));
  });
});
