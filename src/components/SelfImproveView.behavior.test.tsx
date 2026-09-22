// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  proposalList: vi.fn(),
  proposalApply: vi.fn(),
  proposalReject: vi.fn(),
  proposalWithdraw: vi.fn(),
  captureEnabledGet: vi.fn(),
  captureEnabledSet: vi.fn(),
}));

vi.mock("../lib/ipc", () => ({
  proposalList: mocks.proposalList,
  proposalApply: mocks.proposalApply,
  proposalReject: mocks.proposalReject,
  proposalWithdraw: mocks.proposalWithdraw,
  captureEnabledGet: mocks.captureEnabledGet,
  captureEnabledSet: mocks.captureEnabledSet,
}));

import { SelfImproveView } from "./SelfImproveView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

const proposal = {
  id: 7,
  repo: "/repo/a",
  kind: "reflection",
  content: "검증 명령을 먼저 실행한다",
  status: "proposed",
  source_session: "session-1",
  created_at: 1,
  decided_at: null,
  applied_memory_id: null,
};

async function mount(onOpenMemoryCandidates: () => void): Promise<void> {
  await act(async () => {
    root.render(<SelfImproveView onOpenMemoryCandidates={onOpenMemoryCandidates} />);
    await Promise.resolve();
  });
}

function candidateButton(): HTMLButtonElement | undefined {
  return Array.from(container.querySelectorAll("button")).find(
    (button) => button.textContent === "메모리 후보 만들기",
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.proposalList.mockResolvedValue([proposal]);
  mocks.proposalApply.mockResolvedValue(undefined);
  mocks.captureEnabledGet.mockResolvedValue(false);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});

describe("SelfImproveView candidate handoff", () => {
  it("opens the memory candidate list only after proposal apply succeeds", async () => {
    const onOpenMemoryCandidates = vi.fn();
    await mount(onOpenMemoryCandidates);

    await act(async () => candidateButton()?.click());

    expect(mocks.proposalApply).toHaveBeenCalledWith(7);
    expect(onOpenMemoryCandidates).toHaveBeenCalledTimes(1);
  });

  it("stays in review and shows the failure when candidate creation fails", async () => {
    mocks.proposalApply.mockRejectedValue(new Error("candidate write failed"));
    const onOpenMemoryCandidates = vi.fn();
    await mount(onOpenMemoryCandidates);

    await act(async () => candidateButton()?.click());

    expect(onOpenMemoryCandidates).not.toHaveBeenCalled();
    expect(container.textContent).toContain("candidate write failed");
  });
});
