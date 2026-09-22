// @vitest-environment jsdom

/**
 * 실패 카드의 버튼은 목록 재조회가 아니라 그 호스트의 재연결을 부른다. 재조회만으로는
 * 붙지 않은 프로필이 같은 자리에서 다시 거절되므로 버튼이 영영 아무 일도 못 했다.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "./Sidebar";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});

const render = (onRetryHost: (host: string) => Promise<void>) =>
  act(async () =>
    root.render(
      <Sidebar
        browsingHost="local"
        onPickBrowsingHost={() => {}}
        view="home"
        onNewTask={() => {}}
        onQuickLink={() => {}}
        collapsed={false}
        onToggleCollapse={() => {}}
        tasks={[]}
        selectedKey={null}
        projects={[]}
        onOpenTask={() => {}}
        onNewInRepo={() => {}}
        onDeleteTask={() => {}}
        onRemoveProject={() => {}}
        onDiscardOrphans={() => {}}
        hostFailures={[{ host: "mini1", error: "연결 안 됨" }]}
        onRetryHost={onRetryHost}
      />,
    ),
  );

const card = () => container.querySelector<HTMLElement>('[data-host-failure="mini1"]');
const button = () => card()?.querySelector<HTMLButtonElement>("button") ?? null;

describe("사이드바 실패 카드의 다시 연결", () => {
  it("버튼은 그 호스트로 재연결을 부르고, 끝날 때까지 '연결 중'으로 잠긴다", async () => {
    let finish: () => void = () => {};
    const onRetryHost = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    await render(onRetryHost);

    expect(card()?.textContent).toContain("mini1 — 응답 없음");
    expect(button()?.textContent).toBe("다시 연결");

    await act(async () => button()?.click());
    expect(onRetryHost).toHaveBeenCalledWith("mini1");
    expect(button()?.disabled).toBe(true);
    expect(card()?.textContent).toContain("연결 중");

    // 잠긴 동안 다시 눌러도 두 번째 연결을 만들지 않는다.
    await act(async () => button()?.click());
    expect(onRetryHost).toHaveBeenCalledTimes(1);

    await act(async () => { finish(); });
    expect(button()?.disabled).toBe(false);
    expect(card()?.textContent).toContain("응답 없음");
  });

  it("재연결이 실패(reject)해도 카드는 다시 눌릴 수 있는 상태로 돌아온다", async () => {
    const onRetryHost = vi.fn(async () => { throw new Error("터널 실패"); });
    await render(onRetryHost);

    await act(async () => button()?.click());
    await act(async () => { await Promise.resolve(); });

    expect(button()?.disabled).toBe(false);
  });
});
