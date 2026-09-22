// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  insightAvailability: vi.fn(),
  insightEnabledGet: vi.fn(),
  insightEnabledSet: vi.fn(),
  insightWikiFoldersSet: vi.fn(),
  insightNext: vi.fn(),
}));
vi.mock("../../../lib/ipc", () => mocks);
vi.mock("../../../lib/ipc.ts", () => mocks);

import { InsightSection } from "./InsightSection";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CARD = {
  key: "wiki:업무-인프라/vault.md#왜 쓰는가",
  deck: "업무-인프라",
  title: "왜 쓰는가",
  body: "비밀 값을 한곳에서 발급·회수한다.",
  source: "업무-인프라/vault.md › 왜 쓰는가",
  tags: ["infra", "업무-인프라"],
};

const CONNECTED = {
  cards: 12,
  decks: 0,
  deck_cards: 0,
  wiki_notes: 56,
  wiki_cards: 12,
  wiki_root: "/Users/me/지식창고",
  wiki_folders: [
    { path: ".", notes: 2, cards: 1, included: true },
    { path: "개인", notes: 45, cards: 5, included: true },
    { path: "업무-일지", notes: 81, cards: 6, included: true },
  ],
  wiki_scope: null,
  enabled: true,
  warnings: [],
};

const folderBoxes = () =>
  [...container!.querySelectorAll('ul[aria-label="카드로 쓸 폴더"] input[type="checkbox"]')] as HTMLInputElement[];

let container: HTMLDivElement | null = null;
let root: Root | null = null;

const tick = () => act(async () => {});

async function render() {
  await act(async () => root?.render(<InsightSection />));
  await tick();
}

beforeEach(() => {
  for (const fn of Object.values(mocks)) fn.mockReset();
  mocks.insightEnabledGet.mockResolvedValue(true);
  mocks.insightEnabledSet.mockResolvedValue(undefined);
  mocks.insightAvailability.mockResolvedValue(CONNECTED);
  mocks.insightNext.mockResolvedValue(CARD);
  mocks.insightWikiFoldersSet.mockResolvedValue(undefined);
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

describe("InsightSection", () => {
  it("연결된 지식창고의 문서·카드 수를 보여 준다", async () => {
    await render();
    expect(container!.textContent).toContain("지식창고 문서 56편 · 카드 12장");
    expect(container!.textContent).not.toContain("덱 파일");
    expect(container!.textContent).not.toContain("덱 폴더 열기");
    expect(container!.querySelector('[data-setting-id="insight"]')).not.toBeNull();
  });

  it("최상위 폴더를 문서·카드 수와 함께 나열하고, 범위가 없으면 전부 체크된다", async () => {
    await render();
    const boxes = folderBoxes();
    expect(boxes).toHaveLength(3);
    expect(boxes.every((b) => b.checked)).toBe(true);
    expect(container!.textContent).toContain("모든 폴더를 씁니다");
    expect(container!.textContent).toContain("루트 문서");
    expect(container!.textContent).toContain("업무-일지");
    expect(container!.textContent).toContain("문서 81편 · 카드 6장");
  });

  it("폴더 체크를 빼면 나머지 폴더만 범위로 저장하고 가용성을 다시 읽는다", async () => {
    await render();
    const excluded = {
      ...CONNECTED,
      cards: 6,
      wiki_cards: 6,
      wiki_scope: [".", "개인"],
      wiki_folders: CONNECTED.wiki_folders.map((f) =>
        f.path === "업무-일지" ? { ...f, cards: 0, included: false } : f,
      ),
    };
    mocks.insightAvailability.mockResolvedValue(excluded);
    const box = folderBoxes()[2];
    await act(async () => {
      box.click();
    });
    await tick();
    expect(mocks.insightWikiFoldersSet).toHaveBeenCalledWith([".", "개인"]);
    expect(mocks.insightAvailability).toHaveBeenCalledTimes(2);
    const boxes = folderBoxes();
    expect(boxes[2].checked).toBe(false);
    expect(container!.textContent).toContain("체크한 폴더의 문서만 카드가 됩니다");
    expect(container!.textContent).toContain("문서 81편");
    expect(container!.textContent).not.toContain("문서 81편 · 카드");
  });

  it("마지막 폴더까지 다시 켜면 목록 대신 전체(null)로 되돌린다", async () => {
    mocks.insightAvailability.mockResolvedValue({
      ...CONNECTED,
      wiki_scope: [".", "개인"],
      wiki_folders: CONNECTED.wiki_folders.map((f) =>
        f.path === "업무-일지" ? { ...f, cards: 0, included: false } : f,
      ),
    });
    await render();
    await act(async () => {
      folderBoxes()[2].click();
    });
    await tick();
    expect(mocks.insightWikiFoldersSet).toHaveBeenCalledWith(null);
  });

  it("고른 폴더가 없으면 그 사실을 말한다", async () => {
    mocks.insightAvailability.mockResolvedValue({
      ...CONNECTED,
      cards: 0,
      wiki_notes: 0,
      wiki_cards: 0,
      wiki_scope: [],
      wiki_folders: CONNECTED.wiki_folders.map((f) => ({ ...f, cards: 0, included: false })),
    });
    await render();
    expect(folderBoxes().every((b) => !b.checked)).toBe(true);
    expect(container!.textContent).toContain("고른 폴더가 없어 지식창고 카드가 뜨지 않습니다");
    expect(container!.textContent).not.toContain("절(##)이 있는 문서가 아직 없습니다");
  });

  it("창고가 없으면 연결 안내를 하고 미리보기를 잠근다", async () => {
    mocks.insightAvailability.mockResolvedValue({
      ...CONNECTED,
      cards: 0,
      wiki_notes: 0,
      wiki_cards: 0,
      wiki_root: null,
    });
    await render();
    expect(container!.textContent).toContain("지식창고가 연결돼 있지 않습니다");
    const preview = [...container!.querySelectorAll("button")].find((b) => b.textContent === "지금 한 장");
    expect(preview?.disabled).toBe(true);
  });

  it("'지금 한 장'이 카드를 인라인으로 띄운다", async () => {
    await render();
    const preview = [...container!.querySelectorAll("button")].find((b) => b.textContent === "지금 한 장")!;
    expect(preview.disabled).toBe(false);
    await act(async () => preview.click());
    await tick();
    expect(mocks.insightNext).toHaveBeenCalledTimes(1);
    expect(container!.querySelector('section[aria-label="대기 인사이트"]')).not.toBeNull();
    expect(container!.textContent).toContain(CARD.title);
    expect(container!.textContent).toContain(CARD.source);
  });

  it("스위치가 저장에 실패하면 되돌린다", async () => {
    mocks.insightEnabledSet.mockRejectedValue(new Error("db"));
    await render();
    const sw = container!.querySelector('button[role="switch"]') as HTMLButtonElement;
    expect(sw).not.toBeNull();
    const before = sw.getAttribute("aria-checked");
    await act(async () => sw.click());
    await tick();
    expect(sw.getAttribute("aria-checked")).toBe(before);
  });
});
