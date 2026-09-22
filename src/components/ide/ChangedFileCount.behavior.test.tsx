// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  annotationsList: vi.fn(async () => []),
  diffHunks: vi.fn(async () => []),
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
import { ChangedFileCount } from "./ChangesList";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

/** 변경 목록 없이 배지만 그린다 — 기본 상태의 워크스페이스가 이 모습이다. */
const render = async () => {
  await act(async () => {
    root.render(
      <DiffSessionProvider task={{ host: "local", id: 4 }} openDiff={() => {}}>
        <ChangedFileCount>{(count) => <span>{count == null ? "-" : count}</span>}</ChangedFileCount>
      </DiffSessionProvider>,
    );
    await Promise.resolve();
  });
};

beforeEach(() => {
  mocks.taskDiff.mockResolvedValue({
    files: [
      { path: "src/a.ts", status: "M", patch: "@@ -1 +1 @@\n+const b = 2;" },
      { path: "README.md", status: "A", patch: "@@ -0,0 +1 @@\n+hello" },
    ],
    baseline: { kind: "pinned" as const },
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.clearAllMocks();
});

/**
 * 배지는 "변경이 있다"를 알리는 유일한 손잡이다. 배지가 게이트를 열지 않던 동안에는
 * 변경 목록을 한 번 연 뒤에만 숫자가 떴다 — 이미 아는 사람에게만 보이는 알림이었다.
 */
describe("ChangedFileCount 폴링 구독 (설계 §1)", () => {
  it("목록을 열지 않아도 스스로 스냅샷을 불러 수를 그린다", async () => {
    await render();

    expect(mocks.taskDiff).toHaveBeenCalledTimes(1);
    expect(container.textContent).toBe("2");
  });
});
