// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useSessionDraft } from "./useSessionDraft";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const A = "local:1";
const B = "local:2";

let container: HTMLDivElement | null = null;
let root: Root | null = null;
/** 마지막 렌더가 본 초안 — 화면에 그려지는 값이다. */
let shown = "";
/** 마지막 렌더가 준 쓰기 함수 — 사용자의 타이핑에 해당한다. */
let write: (value: string) => void = () => {};
let clearSubmitted: (key: string, expected: string) => void = () => {};

function Probe({ sessionKey }: { sessionKey: string | null }) {
  const [draft, setDraft, consume] = useSessionDraft(sessionKey);
  shown = draft;
  write = setDraft;
  clearSubmitted = consume;
  return <textarea readOnly value={draft} />;
}

const open = async (sessionKey: string | null): Promise<void> => {
  await act(async () => {
    root?.render(<Probe sessionKey={sessionKey} />);
  });
};

const type = async (value: string): Promise<void> => {
  await act(async () => {
    write(value);
  });
};

beforeEach(() => {
  shown = "";
  write = () => {};
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  container = null;
  root = null;
});

describe("useSessionDraft", () => {
  it("late admission clears only the submitted version in its original host", async () => {
    await open("local:late");
    await type("submitted");
    const consume = clearSubmitted;
    await open("remote:late");
    await type("remote draft");
    await act(async () => consume("local:late", "submitted"));
    expect(shown).toBe("remote draft");
    await open("local:late");
    expect(shown).toBe("");
    await type("new draft");
    await act(async () => consume("local:late", "submitted"));
    expect(shown).toBe("new draft");
  });
  it("세션을 옮겼다 돌아오면 쓰던 초안이 그대로 있다", async () => {
    await open(A);
    await type("이 함수 리팩터링 좀");

    await open(B);
    await open(A);

    expect(shown).toBe("이 함수 리팩터링 좀");
  });

  it("다른 세션의 초안이 새지 않는다", async () => {
    await open(A);
    await type("A에 쓰던 문장");

    await open(B);

    expect(shown).toBe("");
  });

  it("각 세션이 자기 초안을 따로 들고 있다", async () => {
    await open(A);
    await type("A 초안");
    await open(B);
    await type("B 초안");

    await open(A);
    expect(shown).toBe("A 초안");
    await open(B);
    expect(shown).toBe("B 초안");
  });

  it("전송해서 비운 세션은 돌아와도 비어 있다", async () => {
    await open(A);
    await type("보낼 질문");
    await type(""); // 전송 성공 → 호출자가 입력창을 비운다

    await open(B);
    await open(A);

    expect(shown).toBe("");
  });

  it("홈(선택 없음)으로 나갔다 돌아와도 초안이 남는다", async () => {
    await open(A);
    await type("쓰다 만 문장");

    await open(null);
    expect(shown).toBe("");

    await open(A);
    expect(shown).toBe("쓰다 만 문장");
  });

  it("id가 같아도 호스트가 다르면 서로 다른 세션이다", async () => {
    await open("local:3");
    await type("로컬 3번에 쓰던 것");

    await open("box:3");

    expect(shown).toBe("");
  });
});
