// @vitest-environment jsdom

import { act, useEffect, useLayoutEffect, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  removeCapture: vi.fn(async () => undefined),
  skillsList: vi.fn(async () => []),
}));

vi.mock("../../lib/ipc", () => ({
  designmodeRemoveCapture: mocks.removeCapture,
  skillsList: mocks.skillsList,
}));

import { AgentComposer } from "./AgentComposer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

/** 래퍼가 밖으로 꺼내 주는 onChange — 훅을 거치지 않은 외부 값 변경(제출 후 비우기)을 흉내 낸다. */
const external: { set: ((next: string) => void) | null } = { set: null };

beforeEach(() => {
  external.set = null;
  swap.to = null;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.useRealTimers();
  vi.clearAllMocks();
});

/** 세션 전환을 밖에서 일으키는 손잡이. */
const swap: { to: ((key: string) => void) | null } = { to: null };

/** useSessionDraft와 같은 모양 — 키가 바뀐 커밋 다음에 값이 바뀌는 두 번째 커밋이 온다. */
function DraftHarness({ initialKey = "a" }: { initialKey?: string }) {
  const [key, setKey] = useState(initialKey);
  const [value, setValue] = useState("");
  const [store] = useState(() => new Map<string, string>());
  useEffect(() => {
    swap.to = setKey;
  }, []);
  useLayoutEffect(() => {
    setValue(store.get(key) ?? "");
  }, [key, store]);
  const write = (next: string): void => {
    setValue(next);
    store.set(key, next);
  };
  return (
    <AgentComposer
      value={value}
      onChange={write}
      draftKey={key}
      onSend={() => {}}
      onInterrupt={() => {}}
      onHistory={() => {}}
      files={[]}
      repo="/repo"
      taskId={7}
      host="local"
    />
  );
}

/** value를 스스로 들고 있는 래퍼 — 되돌리기는 controlled 값이 실제로 되돌아가야 성립한다. */
function Harness({ initial = "" }: { initial?: string }) {
  const [value, setValue] = useState(initial);
  useEffect(() => {
    external.set = setValue;
  }, []);
  return (
    <AgentComposer
      value={value}
      onChange={setValue}
      onSend={() => {}}
      onInterrupt={() => {}}
      onHistory={() => {}}
      files={[]}
      repo="/repo"
      taskId={7}
      host="local"
    />
  );
}

async function renderHarness(opts?: { initial?: string }): Promise<void> {
  await act(async () => {
    root?.render(<Harness initial={opts?.initial} />);
  });
}

function promptField(): HTMLTextAreaElement {
  const field = container?.querySelector<HTMLTextAreaElement>(
    'textarea[placeholder^="에이전트에게 질의"]',
  );
  if (!field) throw new Error("에이전트 질의 입력창을 찾을 수 없습니다");
  return field;
}

const nativeValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;

/** controlled textarea에 사용자 입력을 흉내 낸다 — React value tracker를 우회해야 onChange가 뜬다. */
async function typeValue(next: string): Promise<void> {
  const field = promptField();
  await act(async () => {
    nativeValue.call(field, next);
    field.setSelectionRange(next.length, next.length);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function press(init: KeyboardEventInit): Promise<KeyboardEvent> {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  await act(async () => {
    promptField().dispatchEvent(event);
  });
  return event;
}

/** 연속 타이핑 묶기(800ms)를 시간으로 끊기 위해 Date만 고정한다 — React 스케줄러는 건드리지 않는다. */
function useFakeClock(): void {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(0);
}

function advance(ms: number): void {
  vi.setSystemTime(Date.now() + ms);
}

describe("AgentComposer 되돌리기/다시하기", () => {
  it("타이핑 묶음이 끊긴 뒤 ⌘Z는 직전 묶음으로, ⇧⌘Z는 다시 앞으로 간다", async () => {
    useFakeClock();
    await renderHarness();
    await typeValue("가");
    advance(2000);
    await typeValue("가나");

    await press({ key: "z", metaKey: true });
    expect(promptField().value).toBe("가");

    await press({ key: "z", metaKey: true, shiftKey: true });
    expect(promptField().value).toBe("가나");
  });

  it("Ctrl+Z·⇧Ctrl+Z도 같은 되돌리기·다시하기를 한다", async () => {
    useFakeClock();
    await renderHarness();
    await typeValue("가");
    advance(2000);
    await typeValue("가나");

    await press({ key: "z", ctrlKey: true });
    expect(promptField().value).toBe("가");

    await press({ key: "z", ctrlKey: true, shiftKey: true });
    expect(promptField().value).toBe("가나");
  });

  it("한 번에 들어온 여러 글자는 연속 타이핑과 별개 단계로 되돌아간다", async () => {
    await renderHarness();
    // 두 글자는 연속 타이핑이라 한 묶음, 그다음 붙여넣기는 그 자체로 한 단계다.
    await typeValue("가");
    await typeValue("가나");
    await typeValue("가나붙여넣기");

    await press({ key: "z", metaKey: true });
    expect(promptField().value).toBe("가나");

    await press({ key: "z", metaKey: true });
    expect(promptField().value).toBe("");
  });

  it("부모가 밖에서 비운 값도 ⌘Z로 되돌아온다", async () => {
    await renderHarness({ initial: "이 코드를 검토해줘" });

    await act(async () => external.set?.(""));
    expect(promptField().value).toBe("");

    await press({ key: "z", metaKey: true });
    expect(promptField().value).toBe("이 코드를 검토해줘");
  });

  it("되돌리기 키는 브라우저 기본 동작을 막는다", async () => {
    await renderHarness();

    const event = await press({ key: "z", metaKey: true });

    expect(event.defaultPrevented).toBe(true);
  });

  it("일반 문자 키는 막지 않는다", async () => {
    await renderHarness();

    const event = await press({ key: "z" });

    expect(event.defaultPrevented).toBe(false);
  });

  it("세션이 바뀌면 앞 세션의 문장이 ⌘Z로 새어 들지 않는다", async () => {
    await act(async () => {
      root?.render(<DraftHarness />);
    });
    await typeValue("A 세션 문장");

    await act(async () => swap.to?.("b"));
    expect(promptField().value).toBe("");

    await press({ key: "z", metaKey: true });
    expect(promptField().value).toBe("");
  });

  it("세션을 바꾼 뒤 새로 친 글자는 정상적으로 되돌아간다", async () => {
    await act(async () => {
      root?.render(<DraftHarness />);
    });
    await typeValue("A 세션 문장");
    await act(async () => swap.to?.("b"));

    await typeValue("B");
    await press({ key: "z", metaKey: true });
    expect(promptField().value).toBe("");
  });
});
