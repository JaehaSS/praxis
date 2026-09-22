// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { FilterSegment, type FilterSegmentItem } from "./FilterSegment";

let container: HTMLDivElement;
let root: Root;

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const items: FilterSegmentItem[] = [
  { value: "all", label: "전체", count: 9 },
  { value: "a", label: "가", count: 4 },
  { value: "b", label: "나", count: 5 },
  { value: "c", label: "다", count: 0 },
];

const tabs = () => [...container.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
const labels = () => tabs().map((b) => (b.textContent ?? "").replace(/[\d,]+$/, "").trim());

const mount = async (
  overrides: Partial<Parameters<typeof FilterSegment>[0]> = {},
  onChange: (value: string) => void = () => undefined,
) => {
  await act(async () => {
    root.render(
      <FilterSegment
        label="테스트 범위"
        items={items}
        value="all"
        onChange={onChange}
        alwaysVisible={["all"]}
        {...overrides}
      />,
    );
  });
};

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});

describe("FilterSegment", () => {
  it("개수를 병기하고 0건 칸은 렌더하지 않는다", async () => {
    await mount();

    expect(labels()).toEqual(["전체", "가", "나"]);
    expect(tabs()[0].textContent).toMatch(/9$/);
    expect(tabs()[1].textContent).toMatch(/4$/);
  });

  it("선택된 칸은 0건이어도 남긴다 — 눌린 칸이 사라지면 어디 서 있는지 알 수 없다", async () => {
    await mount({ value: "c" });

    expect(labels()).toEqual(["전체", "가", "나", "다"]);
    expect(tabs()[3].getAttribute("aria-selected")).toBe("true");
  });

  it("고를 것이 하나뿐이면 줄 전체를 그리지 않는다", async () => {
    await mount({
      items: [
        { value: "all", label: "전체", count: 4 },
        { value: "a", label: "가", count: 4 },
        { value: "b", label: "나", count: 0 },
      ],
    });

    expect(tabs()).toEqual([]);
  });

  it("←/→·Home/End로 옮기고 순회에는 선택된 하나만 남는다", async () => {
    let value = "all";
    const rerender = async () => mount({ value }, (next) => { value = next; void rerender(); });
    await rerender();

    const press = async (key: string) => {
      await act(async () => {
        container
          .querySelector('[role="tablist"]')
          ?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
      });
    };

    await press("ArrowRight");
    expect(value).toBe("a");
    expect(tabs().map((b) => b.tabIndex)).toEqual([-1, 0, -1]);

    await press("End");
    expect(value).toBe("b");
    await press("ArrowRight");
    expect(value).toBe("all");
    await press("ArrowLeft");
    expect(value).toBe("b");
    await press("Home");
    expect(value).toBe("all");
  });

  it("활성 칸은 색만이 아니라 배경면으로도 눌림을 보인다", async () => {
    await mount({ value: "a" });

    const active = tabs().find((b) => b.getAttribute("aria-selected") === "true");
    expect(active?.className).toContain("bg-primary/10");
    expect(active?.className).toContain("text-primary-bright");
  });

  it("큰 수는 자릿수 구분자로 읽힌다", async () => {
    await mount({
      items: [
        { value: "all", label: "전체", count: 1_079 },
        { value: "a", label: "가", count: 1_077 },
        { value: "b", label: "나", count: 2 },
      ],
    });

    expect(tabs()[0].textContent).toContain("1,079");
  });
});
