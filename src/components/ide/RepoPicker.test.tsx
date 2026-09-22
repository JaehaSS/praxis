// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./icons", () => ({ Icon: () => null }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { RepoPicker, type RecentRepo } from "./RepoPicker";

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

/** 같은 basename(`foo`)이 두 부모 아래 있다 — 목록만 보고 갈리는지가 A-2.5다. */
const REPOS: RecentRepo[] = [
  { path: "/Users/me/work/praxis", lastUsed: 1 },
  { path: "/Users/me/work/foo", lastUsed: 2 },
  { path: "/Users/me/side/foo", lastUsed: 3 },
  { path: "/Users/me/work/notes", lastUsed: 0 },
];

const chip = (): HTMLButtonElement | null =>
  container?.querySelector<HTMLButtonElement>("button[aria-haspopup='listbox']") ?? null;
const search = (): HTMLInputElement | null =>
  container?.querySelector<HTMLInputElement>("input[role='combobox']") ?? null;
const listbox = (): HTMLElement | null =>
  container?.querySelector<HTMLElement>("[role='listbox']") ?? null;
const options = (): HTMLElement[] => [
  ...(container?.querySelectorAll<HTMLElement>("[role='option']") ?? []),
];
const browse = (): HTMLButtonElement | undefined =>
  [...(container?.querySelectorAll<HTMLButtonElement>("button") ?? [])].find((b) =>
    b.textContent?.includes("폴더 열기"),
  );

async function open(
  recentRepos: RecentRepo[] = REPOS,
  onPick: (p: string) => void = () => undefined,
  repo = "",
): Promise<void> {
  await act(async () => {
    root?.render(<RepoPicker repo={repo} recentRepos={recentRepos} onPick={onPick} />);
  });
  await act(async () => chip()?.click());
}

/** React가 value setter를 가로채므로 native setter로 넣고 input 이벤트를 올린다. */
async function type(text: string): Promise<void> {
  const input = search();
  if (!input) throw new Error("검색 필드가 없다");
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, text);
  await act(async () => {
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function press(key: string): Promise<void> {
  await act(async () => {
    search()?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
}

describe("RepoPicker 검색", () => {
  it("레포가 하나뿐이어도 검색 필드는 같은 자리를 지킨다", async () => {
    await open([REPOS[0]]);
    expect(search()).not.toBeNull();
  });

  it("좁혀진 정도를 표시/전체 카운트로 보여준다", async () => {
    await open();
    expect(container?.textContent).toContain("4");
    await type("foo");
    expect(container?.textContent).toContain("2/4");
  });

  it("경로 조각으로 찾는다 — 매칭은 basename이 아니라 전체 경로에 걸린다", async () => {
    await open();
    await type("work/foo");
    expect(options().map((o) => o.getAttribute("title"))).toEqual(["/Users/me/work/foo"]);
  });

  it("같은 이름의 레포는 부모 경로로 갈린다", async () => {
    await open();
    await type("foo");
    const texts = options().map((o) => o.textContent ?? "");
    expect(texts).toHaveLength(2);
    expect(texts[0]).toContain("/Users/me/work");
    expect(texts[1]).toContain("/Users/me/side");
  });

  it("↓와 Enter로 커서가 짚은 레포를 확정하고 닫는다", async () => {
    const onPick = vi.fn();
    await open(REPOS, onPick);
    await type("foo");
    await press("ArrowDown");
    await press("Enter");
    expect(onPick).toHaveBeenCalledWith("/Users/me/side/foo");
    expect(search()).toBeNull();
  });

  it("커서는 목록 경계에서 순환하지 않는다", async () => {
    const onPick = vi.fn();
    await open(REPOS, onPick);
    await type("work/foo");
    await press("ArrowDown");
    await press("ArrowUp");
    await press("ArrowUp");
    await press("Enter");
    expect(onPick).toHaveBeenCalledWith("/Users/me/work/foo");
  });

  it("Esc는 질의를 비우는 단계 없이 곧바로 닫는다", async () => {
    await open();
    await type("foo");
    await press("Escape");
    expect(search()).toBeNull();
  });

  it("combobox·listbox·option과 커서를 aria로 잇는다", async () => {
    await open();
    await type("foo");
    expect(listbox()).not.toBeNull();
    expect(search()?.getAttribute("aria-controls")).toBe("repo-picker-list");
    expect(search()?.getAttribute("aria-activedescendant")).toBe("repo-opt-0");
    await press("ArrowDown");
    expect(search()?.getAttribute("aria-activedescendant")).toBe("repo-opt-1");
    expect(options()[1]?.id).toBe("repo-opt-1");
  });

  it("매치가 0건이면 왜 없는지와 다음 단서를 준다", async () => {
    await open();
    await type("없는레포");
    expect(options()).toHaveLength(0);
    expect(container?.textContent).toContain("'없는레포'와 일치하는 최근 레포가 없습니다");
    expect(container?.textContent).toContain("폴더 열기…로 다른 레포를 고를 수 있습니다");
  });

  it("'폴더 열기…'는 후보가 아니라 탈출구다 — 검색에 걸리지 않고 늘 남는다", async () => {
    await open();
    expect(browse()?.getAttribute("role")).toBeNull();
    await type("없는레포");
    expect(browse()).not.toBeUndefined();
    await type("폴더 열기");
    expect(options()).toHaveLength(0);
    expect(browse()).not.toBeUndefined();
  });
});

describe("RepoPicker 배치", () => {
  const menu = (): HTMLElement | null =>
    listbox()?.closest<HTMLElement>("div.absolute") ?? null;

  it("기본은 Composer처럼 위·왼쪽으로 연다", async () => {
    await open();
    expect(menu()?.className).toContain("bottom-full");
    expect(menu()?.className).toContain("left-0");
  });

  it("placement=down·align=right면 아래·오른쪽으로 열어 화면 상단·우측을 벗어나지 않는다", async () => {
    await act(async () => {
      root?.render(
        <RepoPicker repo="" recentRepos={REPOS} onPick={() => undefined} placement="down" align="right" />,
      );
    });
    await act(async () => chip()?.click());
    expect(menu()?.className).toContain("top-full");
    expect(menu()?.className).not.toContain("bottom-full");
    expect(menu()?.className).toContain("right-0");
    expect(menu()?.className).not.toContain("left-0");
  });
});
