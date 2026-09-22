// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "../../lib/ipc";
import { ContextResetButton } from "./ContextResetButton";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// 부분 mock — 모듈 전체를 대체하면 이 트리 아래가 import하는 다른 IPC가 통째로 사라진다.
const ipc = vi.hoisted(() => ({ convoContextReset: vi.fn() }));
vi.mock("../../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/ipc")>()),
  convoContextReset: ipc.convoContextReset,
}));

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const task = (over: Partial<Task> = {}): Task =>
  ({
    id: 7,
    repo: "/r",
    branch: "b",
    base: "dev",
    worktree_path: "/w",
    instruction: "i",
    state: "AwaitingReview",
    created_at: 0,
    updated_at: 0,
    agent: "claude",
    mode: "conversation",
    ...over,
  }) as Task;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  ipc.convoContextReset.mockReset().mockResolvedValue(undefined);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.restoreAllMocks();
});

const button = () =>
  container?.querySelector<HTMLButtonElement>('button[aria-label="컨텍스트 비우기"]') ?? null;

const click = async () => {
  const el = button();
  expect(el).toBeTruthy();
  await act(async () => el?.click());
};

const render = async (t: Task = task(), onReset = () => undefined) => {
  await act(async () => root?.render(<ContextResetButton task={t} onReset={onReset} />));
};

describe("ContextResetButton", () => {
  it("확인을 취소하면 아무것도 하지 않는다 — 되돌릴 수 없는 동작이다", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    const onReset = vi.fn();
    await render(task(), onReset);

    await click();

    expect(confirm).toHaveBeenCalledOnce();
    expect(ipc.convoContextReset).not.toHaveBeenCalled();
    expect(onReset).not.toHaveBeenCalled();
  });

  it("확인 문구가 남는 것과 사라지는 것을 모두 말한다", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    await render();

    await click();

    const text = String(confirm.mock.calls[0]?.[0]);
    // "컨텍스트를 비웁니다"만으로는 코드까지 날아가는지 알 수 없다.
    expect(text).toContain("캡슐");
    expect(text).toContain("코드·worktree·컨텍스트 파일은 그대로");
    expect(text).toContain("되돌릴 수 없습니다");
  });

  it.each(["Done", "Discarded", "Failed"])("%s 작업에는 버튼을 내보내지 않는다", async (state) => {
    // 이어질 턴이 없는 작업에 캡슐을 남기면 아무도 읽지 않는다. 백엔드도 같은 셋을 거부한다.
    await render(task({ state }));
    expect(button()).toBeNull();
  });

  it("승인하면 절단하고 부모에게 알린다", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const alert = vi.spyOn(window, "alert").mockImplementation(() => undefined);
    const onReset = vi.fn();
    await render(task(), onReset);

    await click();

    expect(ipc.convoContextReset).toHaveBeenCalledWith(7);
    expect(onReset).toHaveBeenCalledOnce();
    // 캡슐이 어디로 가는지 알려야 한다 — 파일이 아니라 다음 메시지다(ADR 0170).
    expect(String(alert.mock.calls[0]?.[0])).toContain("다음 메시지");
    // 갱신이 alert보다 먼저다 — alert는 블로킹이라 뒤집히면 확인을 누를 때까지 화면이 낡는다.
    expect(onReset.mock.invocationCallOrder[0]).toBeLessThan(alert.mock.invocationCallOrder[0]);
  });

  it("실패하면 세션이 그대로임을 알린다 — 백엔드의 순서 계약을 사용자에게도 전한다", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const alert = vi.spyOn(window, "alert").mockImplementation(() => undefined);
    ipc.convoContextReset.mockRejectedValue("워크트리 디렉터리가 없어 저장할 수 없습니다");
    const onReset = vi.fn();
    await render(task(), onReset);

    await click();

    const text = String(alert.mock.calls[0]?.[0]);
    expect(text).toContain("세션은 그대로입니다");
    expect(text).toContain("워크트리 디렉터리가 없어");
    expect(onReset).not.toHaveBeenCalled();
  });

  it("실패해도 잠금이 풀려 다시 시도할 수 있다", async () => {
    // `disabled={busy}` 때문에 "연타가 막히는가"는 jsdom에서 자명하게 통과한다. 정작 깨지기
    // 쉬운 쪽은 반대다 — `.finally`가 사라지면 한 번 실패한 버튼이 영구히 잠긴다.
    vi.spyOn(window, "confirm").mockReturnValue(true);
    vi.spyOn(window, "alert").mockImplementation(() => undefined);
    ipc.convoContextReset.mockRejectedValueOnce("일시 오류").mockResolvedValue(undefined);
    await render();

    await click();
    expect(button()?.disabled).toBe(false);
    await click();

    expect(ipc.convoContextReset).toHaveBeenCalledTimes(2);
  });
});
