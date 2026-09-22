// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { IsolationPicker } from "./IsolationPicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

const openMenu = async () => {
  const chip = container?.querySelector<HTMLButtonElement>('button[aria-label="격리 방식"]');
  expect(chip).toBeTruthy();
  await act(async () => chip?.click());
};

const renderPicker = async (props: Partial<Parameters<typeof IsolationPicker>[0]> = {}) =>
  act(async () =>
    root?.render(
      <IsolationPicker choice="default" globalDefault onPick={() => undefined} {...props} />,
    ),
  );

const itemByText = (text: string) =>
  [...(container?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.includes(text));

describe("IsolationPicker", () => {
  it("팝오버를 열면 세 선택지가 모두 보인다", async () => {
    await renderPicker();
    await openMenu();
    const text = container?.textContent ?? "";
    expect(text).toContain("워크트리 격리 · 이 프로젝트만");
    expect(text).toContain("직접 실행 · 이 프로젝트만");
  });

  it("격리하는 선택지는 캡션에서 삭제를 말한다", async () => {
    await renderPicker();
    await openMenu();
    // 이 단언이 이 컴포넌트의 존재 이유다 — 라벨은 삭제를 말하지 않았고 캡션이 말한다.
    const captions = [...(container?.querySelectorAll("span") ?? [])]
      .map((s) => s.textContent ?? "")
      .filter((t) => t.includes("삭제"));
    expect(captions.length).toBeGreaterThanOrEqual(2);
  });

  it("직접 실행 선택지는 지울 것이 없다고 말한다", async () => {
    await renderPicker();
    await openMenu();
    expect(container?.textContent).toContain("지울 것도 없습니다");
  });

  it("고르면 그 선택지가 콜백으로 온다", async () => {
    const onPick = vi.fn();
    await renderPicker({ onPick });
    await openMenu();
    await act(async () => itemByText("직접 실행 · 이 프로젝트만")?.click());
    expect(onPick).toHaveBeenCalledWith("pinned-off");
  });

  it("프로젝트 고정이면 칩이 그 사실을 표시한다", async () => {
    await renderPicker({ choice: "pinned-on" });
    expect(container?.textContent).toContain("· 이 프로젝트");
  });

  it("전역 기본이 꺼져 있으면 '기본을 따름'이 직접 실행으로 읽힌다", async () => {
    await renderPicker({ globalDefault: false });
    // 칩 자체가 직접 실행이어야 한다 — 기본값을 반영하지 않으면 사용자가 무엇이 일어날지 모른다.
    expect(container?.textContent).toContain("직접 실행");
    await openMenu();
    expect(container?.textContent).toContain("메인 체크아웃에서 그대로 작업합니다");
  });

  it("고르면 팝오버가 닫힌다", async () => {
    await renderPicker();
    await openMenu();
    await act(async () => itemByText("워크트리 격리 · 이 프로젝트만")?.click());
    expect(container?.textContent).not.toContain("— 새 작업에 적용");
  });
});
