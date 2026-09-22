// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DebateView } from "./DebateView";
import type { DebateEventLike } from "./debate-rounds";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// 마크다운 렌더는 이 테스트의 책임이 아니다 — 면에 무엇이 실렸는지만 본다.
vi.mock("./Markdown", () => ({
  Markdown: ({ text }: { text: string }) => <div data-testid="md">{text}</div>,
}));

// 부분 mock이어야 한다 — 통째로 대체하면 이 트리 아래가 쓰는 다른 IPC까지 사라진다.
const ipc = vi.hoisted(() => ({ convoInterrupt: vi.fn(), debateEnd: vi.fn() }));
vi.mock("../../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/ipc")>()),
  convoInterrupt: ipc.convoInterrupt,
  debateEnd: ipc.debateEnd,
}));

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const events: DebateEventLike[] = [
  { kind: "user", text: "어느 쪽인가?" },
  { kind: "text", text: "A안을 권한다", speaker: "left" },
  { kind: "text", text: "전제가 틀렸다", speaker: "right" },
];

const view = (over: Partial<React.ComponentProps<typeof DebateView>> = {}) => (
  <DebateView
    taskId={7}
    events={events}
    roundCap={3}
    left={{ agent: "claude", model: "sonnet-4.6" }}
    right={{ agent: "codex", model: "gpt-5.2" }}
    busy={false}
    onSend={() => true}
    onEnded={() => undefined}
    {...over}
  />
);

const render = async (node: React.ReactElement) => {
  await act(async () => root?.render(node));
};

const typeDraft = async (text: string) => {
  const input = container?.querySelector("textarea");
  if (!input) throw new Error("토론 입력이 없다");
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  await act(async () => {
    setter?.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  return input;
};

const byText = (text: string) =>
  [...(container?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.trim() === text);

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  ipc.convoInterrupt.mockReset().mockResolvedValue(undefined);
  ipc.debateEnd.mockReset().mockResolvedValue(undefined);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.restoreAllMocks();
});

describe("DebateView", () => {
  it("면 헤더에 에이전트 이름·모델 배지와 발화자 접근성 레이블을 단다", async () => {
    await render(view());
    const left = container?.querySelector('[aria-label="좌측 발화자 Claude Code"]');
    const right = container?.querySelector('[aria-label="우측 발화자 Codex"]');
    expect(left?.textContent).toContain("sonnet-4.6");
    expect(right?.textContent).toContain("gpt-5.2");
    // 라운드 구분선은 목록 구분이 아니라 제목 수준이다 — 안 들리면 두 면이 한 흐름으로 읽힌다.
    const heading = container?.querySelector('[role="heading"]');
    expect(heading?.textContent).toContain("라운드 1/3");
  });

  it("라운드가 도는 동안 컴포저는 잠기고 중단만 남는다", async () => {
    await render(view({ busy: true }));
    const input = container?.querySelector("textarea");
    expect(input?.disabled).toBe(true);
    expect(byText("전송")).toBeUndefined();
    await act(async () => byText("중단")?.click());
    expect(ipc.convoInterrupt).toHaveBeenCalledWith(7);
  });

  it("보내는 쪽이 거절하면 초안을 지우지 않는다 — 삼킨 발화는 다시 칠 수 없다", async () => {
    const rejected = vi.fn(() => false);
    await render(view({ onSend: rejected }));
    const input = await typeDraft("이어서 물어볼 것");
    await act(async () => byText("전송")?.click());
    expect(rejected).toHaveBeenCalledWith("이어서 물어볼 것");
    expect(input.value).toBe("이어서 물어볼 것");

    const accepted = vi.fn(() => true);
    await render(view({ onSend: accepted }));
    await act(async () => byText("전송")?.click());
    expect(accepted).toHaveBeenCalled();
    expect(container?.querySelector("textarea")?.value).toBe("");
  });

  it("합의 배너에 결론 복사와 토론 끝내기가 있고, 끝내면 우측 자리를 버린다", async () => {
    const ended = vi.fn();
    await render(view({ events: [...events, { kind: "debate_ended", reason: "consensus" }], onEnded: ended }));
    expect(container?.textContent).toContain("합의했습니다");
    expect(byText("결론 복사")).toBeDefined();
    await act(async () => byText("토론 끝내기")?.click());
    expect(ipc.debateEnd).toHaveBeenCalledWith(7);
    expect(ended).toHaveBeenCalled();
  });

  it("상한으로 끝나면 배너 문구만 다르고 결론 복사는 없다 — 복사할 결론이 없다", async () => {
    await render(view({ events: [...events, { kind: "debate_ended", reason: "round_cap" }] }));
    expect(container?.textContent).toContain("합의하지 못했습니다");
    expect(byText("결론 복사")).toBeUndefined();
    // 끝난 뒤 컴포저는 되살아난다 — 다음 발화가 곧 계속이다.
    expect(container?.querySelector("textarea")?.disabled).toBe(false);
  });
});
