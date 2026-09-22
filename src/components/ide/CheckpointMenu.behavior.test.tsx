// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  create: vi.fn(),
  list: vi.fn(),
  rewind: vi.fn(),
}));

vi.mock("../../lib/ipc", () => ({
  checkpointCreate: mocks.create,
  checkpointList: mocks.list,
  convoRewind: mocks.rewind,
}));

import { CheckpointMenu } from "./CheckpointMenu";
import type { ConvoCheckpoint } from "../../lib/ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const point = (id: number, label: string): ConvoCheckpoint => ({
  id,
  task_id: 3,
  label,
  worktree_commit: `sha-${id}`,
  convo_event_max_id: id * 10,
  ts: 1_700_000_000 + id,
});

let container: HTMLDivElement;
let root: Root | null = null;
let onRewound: ReturnType<typeof vi.fn>;

const trigger = (): HTMLButtonElement => {
  const button = container.querySelector<HTMLButtonElement>('button[aria-label="체크포인트"]');
  if (!button) throw new Error("체크포인트 트리거가 없다");
  return button;
};

const buttonByText = (text: string): HTMLButtonElement => {
  const found = [...container.querySelectorAll("button")].find((b) => b.textContent === text);
  if (!found) throw new Error(`버튼을 찾지 못했다: ${text}`);
  return found as HTMLButtonElement;
};

const click = async (el: HTMLElement): Promise<void> => {
  await act(async () => {
    el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
};

async function render(taskId = 3): Promise<void> {
  await act(async () => {
    root?.render(<CheckpointMenu taskId={taskId} onRewound={onRewound} />);
  });
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  onRewound = vi.fn();
  mocks.list.mockReset();
  mocks.create.mockReset();
  mocks.rewind.mockReset();
  mocks.list.mockResolvedValue([point(2, "파서 교체 직전"), point(1, "착수")]);
  mocks.create.mockResolvedValue(point(3, "새 지점"));
  mocks.rewind.mockResolvedValue({ kept: "요약 본문", abandoned: ["정규식 파서"] });
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  container.remove();
  vi.restoreAllMocks();
});

describe("CheckpointMenu", () => {
  it("닫힌 채로는 개수만 알리고 대화 영역에 아무것도 펼치지 않는다", async () => {
    await render();

    expect(trigger().textContent).toContain("2");
    expect(container.querySelector("input")).toBeNull();
    expect(container.textContent).not.toContain("여기로 되감기");
  });

  it("트리거를 누르면 팝오버가 열리고 Esc로 닫힌다", async () => {
    await render();
    await click(trigger());

    expect(container.querySelector("input")).not.toBeNull();
    expect(container.textContent).toContain("파서 교체 직전");

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });

    expect(container.querySelector("input")).toBeNull();
  });

  it("바깥을 누르면 닫힌다", async () => {
    await render();
    await click(trigger());

    await act(async () => {
      document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    });

    expect(container.querySelector("input")).toBeNull();
  });

  it("되감기는 확인을 한 번 거친 뒤에야 실행되고 요약을 남긴다", async () => {
    await render();
    await click(trigger());
    await click(buttonByText("여기로 되감기"));

    expect(mocks.rewind).not.toHaveBeenCalled();
    expect(container.textContent).toContain("이후 기록을 버립니다");

    await click(buttonByText("되감기"));

    expect(mocks.rewind).toHaveBeenCalledWith(3, 2);
    expect(onRewound).toHaveBeenCalledTimes(1);
    expect(container.textContent).toContain("요약 본문");
    expect(container.textContent).toContain("정규식 파서");
  });

  it("작업을 바꾸면 이전 작업의 되감기 결과를 끌고 가지 않는다", async () => {
    await render();
    await click(trigger());
    await click(buttonByText("여기로 되감기"));
    await click(buttonByText("되감기"));
    expect(container.textContent).toContain("요약 본문");

    await render(4);

    expect(mocks.list).toHaveBeenLastCalledWith(4);
    expect(container.textContent).not.toContain("요약 본문");
  });
});
