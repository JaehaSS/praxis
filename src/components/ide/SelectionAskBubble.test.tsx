// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SelectionAskBubble, type BubbleAnchor } from "./SelectionAskBubble";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const anchor: BubbleAnchor = {
  top: 100,
  left: 40,
  placement: "below",
  startLine: 3,
  endLine: 3,
};

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const mount = async (props: Partial<Parameters<typeof SelectionAskBubble>[0]> = {}) => {
  const merged = {
    anchor,
    busy: false,
    error: null,
    onSubmit: vi.fn(),
    onAttachOnly: vi.fn(),
    onClose: vi.fn(),
    ...props,
  };
  await act(async () => {
    root?.render(<SelectionAskBubble {...merged} />);
    await Promise.resolve();
  });
  return merged;
};

const textarea = () => container?.querySelector("textarea");
const buttonWith = (text: string) =>
  [...(container?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.includes(text));
/** 칩을 눌러 입력창을 펼친다 — 처음에는 접혀 있다(포커스를 뺏지 않으려고). */
const expand = async () => {
  await act(async () => buttonWith("질문")?.click());
};
const type = async (value: string) => {
  const ta = textarea();
  if (!ta) throw new Error("입력창이 없다");
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(
      globalThis.HTMLTextAreaElement.prototype,
      "value",
    )?.set;
    setter?.call(ta, value);
    ta.dispatchEvent(new Event("input", { bubbles: true }));
  });
};
const pressEnter = async () => {
  await act(async () => {
    textarea()?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    );
  });
};

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  container = null;
  root = null;
  vi.clearAllMocks();
});

describe("SelectionAskBubble", () => {
  it("선택 줄 범위를 보여 준다", async () => {
    await mount();
    expect(container?.textContent).toContain("L3");

    await mount({ anchor: { ...anchor, endLine: 12 } });
    expect(container?.textContent).toContain("L3–L12");
  });

  it("Enter로 질문을 보내고 입력을 비운다", async () => {
    const props = await mount();
    await expand();
    await type("이게 왜 필요해?");
    await pressEnter();

    expect(props.onSubmit).toHaveBeenCalledWith("이게 왜 필요해?");
    expect(textarea()?.value).toBe("");
  });

  it("빈 질문은 보내지 않는다", async () => {
    const props = await mount();
    await expand();
    await type("   ");
    await pressEnter();

    expect(props.onSubmit).not.toHaveBeenCalled();
  });

  it("응답 중이면 잠그고 그 이유를 적는다", async () => {
    // 큐잉하면 사용자가 언제 갈지 모르는 메시지를 기다리게 된다.
    const props = await mount({ busy: true });
    await expand();
    expect(textarea()?.disabled).toBe(true);
    expect(textarea()?.placeholder).toContain("응답 중");

    await pressEnter();
    expect(props.onSubmit).not.toHaveBeenCalled();
  });

  it("전송에 실패해도 입력을 잃지 않는다", async () => {
    await mount({ error: "전송 실패: 연결 없음" });
    await expand();
    await type("살아남아야 하는 질문");

    expect(container?.textContent).toContain("전송 실패: 연결 없음");
    expect(textarea()?.value).toBe("살아남아야 하는 질문");
  });

  it("보조 액션은 ⌘L과 같은 첨부다", async () => {
    // 버블은 ⌘L을 대체하지 않는다 — 보이지 않던 동선을 보이게 만들 뿐이다.
    const props = await mount();
    // 펼치지 않은 칩에서 바로 눌린다 — 첨부만 하려고 입력창을 열 이유가 없다.
    await act(async () => buttonWith("⌘L")?.click());

    expect(props.onAttachOnly).toHaveBeenCalledOnce();
  });

  it("선택 직후에는 입력창을 띄우지 않는다 — 포커스를 뺏으면 ⌘C가 죽는다", async () => {
    await mount();

    expect(textarea()).toBeFalsy();
    // 드래그를 끝낸 손의 포커스가 그대로 남아 있어야 이어서 복사할 수 있다.
    expect(document.activeElement).toBe(document.body);
  });

  it("질문을 눌러야 입력창이 열리고, 그때는 포커스를 가져간다", async () => {
    await mount();
    await expand();

    expect(textarea()).toBeTruthy();
    expect(document.activeElement).toBe(textarea());
  });

  it("새 선택이 오면 다시 접힌다", async () => {
    await mount();
    await expand();
    expect(textarea()).toBeTruthy();

    // 같은 버블이 다른 범위를 가리키게 된다 — 드래그를 새로 한 경우다.
    await mount({ anchor: { ...anchor, startLine: 9, endLine: 11 } });

    expect(textarea()).toBeFalsy();
  });

  it("Esc로 닫는다", async () => {
    const props = await mount();
    await expand();
    await act(async () => {
      container
        ?.querySelector("[role=dialog]")
        ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });

    expect(props.onClose).toHaveBeenCalledOnce();
  });
});
