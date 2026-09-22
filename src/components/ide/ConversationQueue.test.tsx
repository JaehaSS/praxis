// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ConversationQueue } from "./ConversationQueue";
import type { ConversationQueueSnapshot } from "../../lib/conversation-queue";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let container: HTMLDivElement;
beforeEach(() => { container = document.createElement("div"); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); });

it("shows FIFO previews, full text, images and per-item cancellation, protecting unresolved submissions", async () => {
  const onRemove = vi.fn();
  const onPause = vi.fn();
  const queue: ConversationQueueSnapshot = { paused: false, reason: null, items: [
    { id: "a", message: "first\nfull reference", images: ["a.png"], sending: false, uncertain: false },
    { id: "b", message: "second", images: [], sending: true, uncertain: false },
    { id: "c", message: "third", images: [], sending: false, uncertain: true },
  ] };
  await act(async () => root.render(<ConversationQueue queue={queue} connected onRemove={onRemove} onPause={onPause} onResume={vi.fn()} />));
  expect(container.querySelectorAll("li")).toHaveLength(3);
  expect(container.textContent).toContain("full reference");
  expect(container.textContent).toContain("이미지 1개");
  expect(container.textContent).toContain("접수 확인 필요");
  expect(container.querySelectorAll('button[aria-label*="삭제"]')).toHaveLength(1);
  await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="대기 요청 1 삭제"]')!.click());
  expect(onRemove).toHaveBeenCalledWith("a");
  await act(async () => container.querySelector("button")!.click());
  expect(onPause).toHaveBeenCalledOnce();
});

it("shows the reason for pausing and disables resume while disconnected", async () => {
  const queue: ConversationQueueSnapshot = { paused: true, reason: "연결 끊김", items: [
    { id: "a", message: "first", images: [], sending: false, uncertain: false },
  ] };
  const onResume = vi.fn();
  await act(async () => root.render(<ConversationQueue queue={queue} connected={false} onRemove={vi.fn()} onPause={vi.fn()} onResume={onResume} />));
  expect(container.querySelector('[role="alert"]')?.textContent).toBe("연결 끊김");
  expect(container.querySelector("button")!.disabled).toBe(true);
  expect(container.textContent).toContain("앱을 종료하면 대기열이 사라집니다");
  await act(async () => root.render(<ConversationQueue queue={queue} connected onRemove={vi.fn()} onPause={vi.fn()} onResume={onResume} />));
  await act(async () => container.querySelector("button")!.click());
  expect(onResume).toHaveBeenCalledOnce();
});
