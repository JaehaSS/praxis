// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ acknowledge: vi.fn() }));
vi.mock("../../lib/notifications", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/notifications")>()),
  notificationAcknowledge: mocks.acknowledge,
}));

import { NotificationInbox } from "./NotificationInbox";
import type { NotificationSnapshot } from "../../lib/notifications";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const snapshot: NotificationSnapshot = {
  enabled: true,
  delivery_error: null,
  sources: [],
  items: [{ sequence: 8, task_id: 4, ts: 1, kind: "result", title: "테스트 결과", repo: "/work/demo", host: "local", source_id: "source", read_sequence: 0 }],
};

let container: HTMLDivElement;
let root: Root;

const button = (label: string) => [...container.querySelectorAll("button")].find((item) => item.textContent === label) as HTMLButtonElement;

beforeEach(() => {
  mocks.acknowledge.mockReset();
  mocks.acknowledge.mockResolvedValue({ ...snapshot, items: [] });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

async function render(onResult = vi.fn(async () => true), onChanges = vi.fn(async () => {})) {
  await act(async () => root.render(<NotificationInbox snapshot={snapshot} error={null} onRetry={() => {}} onSnapshot={() => {}} onResult={onResult} onChanges={onChanges} />));
  await act(async () => button("미확인 1건").click());
  return { onResult, onChanges };
}

describe("NotificationInbox", () => {
  it("결과가 성공적으로 열렸을 때 해당 sequence까지만 확인한다", async () => {
    const { onResult } = await render();
    await act(async () => button("결과 보기").click());
    expect(onResult).toHaveBeenCalledWith(snapshot.items[0]);
    expect(mocks.acknowledge).toHaveBeenCalledWith("local", "source", 4, 8);
  });

  it("변경 보기는 확인 기록을 쓰지 않는다", async () => {
    const { onChanges } = await render();
    await act(async () => button("변경 보기").click());
    expect(onChanges).toHaveBeenCalledWith(snapshot.items[0]);
    expect(mocks.acknowledge).not.toHaveBeenCalled();
  });

  it("결과를 열지 못하면 확인하지 않는다", async () => {
    await render(vi.fn(async () => false));
    await act(async () => button("결과 보기").click());
    expect(mocks.acknowledge).not.toHaveBeenCalled();
    expect(container.querySelector('[role="status"]')?.textContent).toContain("확인");
  });

  it("확인 저장 실패를 목록 안 오류로 남긴다", async () => {
    mocks.acknowledge.mockRejectedValueOnce(new Error("저장 실패"));
    await render();
    await act(async () => button("확인").click());
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("저장 실패");
  });

  it("결과 이동 실패를 목록 안 오류로 남긴다", async () => {
    await render(vi.fn(async () => { throw new Error("불러오기 실패"); }));
    await act(async () => button("결과 보기").click());
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("불러오기 실패");
  });
});
