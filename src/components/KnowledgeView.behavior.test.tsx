// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  vaultsGet: vi.fn(),
  vaultsSet: vi.fn(),
  sync: vi.fn(),
}));

vi.mock("../lib/ipc", () => ({
  knowledgeVaultsGet: mocks.vaultsGet,
  knowledgeVaultsSet: mocks.vaultsSet,
  knowledgeSync: mocks.sync,
}));
vi.mock("./WikiDocumentsPanel", () => ({
  WikiDocumentsPanel: ({ canAttach }: { canAttach: boolean }) => (
    <p>{`문서 자료 패널 ${canAttach ? "붙이기 가능" : "읽기만"}`}</p>
  ),
}));
// 대기 인사이트 섹션은 제 테스트(`InsightSection.test.tsx`)가 있다 — 여기서는 자리만 확인한다.
vi.mock("./ide/settings/InsightSection", () => ({
  InsightSection: () => <p>대기 인사이트 섹션</p>,
}));
vi.mock("./ide/DirectoryPickerModal", () => ({
  DirectoryPickerModal: ({ onPick }: { onPick: (p: string) => void }) => (
    <button data-testid="pick" onClick={() => onPick("/Users/me/Obsidian Vault")}>
      고르기
    </button>
  ),
}));

import { KnowledgeView } from "./KnowledgeView";

let container: HTMLDivElement;
let root: Root;

const mount = async () => {
  await act(async () => {
    root.render(<KnowledgeView />);
  });
};

const click = async (el: Element | null) => {
  expect(el, "클릭 대상이 없다").not.toBeNull();
  await act(async () => {
    (el as HTMLElement).click();
  });
};

const buttonByText = (text: string) =>
  Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes(text)) ??
  null;

beforeEach(() => {
  vi.clearAllMocks();
  mocks.vaultsGet.mockResolvedValue({ vaults: [] });
  mocks.vaultsSet.mockResolvedValue(undefined);
  mocks.sync.mockResolvedValue({ indexed: 3, skipped: 1, deleted: 0, edges: 2, embedded: 5 });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("KnowledgeView", () => {
  it("연결된 vault가 없으면 동기화를 막는다", async () => {
    // 대상이 없는데 누르게 두면 사용자는 에러 메시지로만 상황을 알게 된다.
    await mount();
    expect((buttonByText("동기화") as HTMLButtonElement).disabled).toBe(true);
  });

  it("폴더를 고르면 목록에 추가하고 저장한다", async () => {
    await mount();
    await click(buttonByText("폴더 추가"));
    await click(container.querySelector('[data-testid="pick"]'));

    expect(mocks.vaultsSet).toHaveBeenCalledWith([
      expect.objectContaining({ root: "/Users/me/Obsidian Vault" }),
    ]);
    expect(container.textContent).toContain("/Users/me/Obsidian Vault");
  });

  it("새 vault에는 Claude Code 임베딩 제외가 기본으로 붙는다", async () => {
    // 실측에서 세션 로그가 청크의 98%였고 전량 임베딩에 ~6시간이 든다.
    // 기본값이 없으면 첫 동기화에서 사용자가 그 비용을 그대로 맞는다.
    await mount();
    await click(buttonByText("폴더 추가"));
    await click(container.querySelector('[data-testid="pick"]'));

    const saved = mocks.vaultsSet.mock.calls[0][0] as { embed_exclude: string[] }[];
    expect(saved[0].embed_exclude).toContain("Claude Code/**");
  });

  it("동기화 결과를 건수로 보여준다", async () => {
    mocks.vaultsGet.mockResolvedValue({
      vaults: [{ root: "/v", exclude: [], embed_exclude: [] }],
    });
    await mount();
    await click(buttonByText("동기화"));

    expect(container.textContent).toContain("색인 3");
    expect(container.textContent).toContain("임베딩 5");
  });

  it("동기화 실패를 화면에 남긴다", async () => {
    // 조용히 실패하면 사용자는 검색이 안 되는 이유를 알 수 없다.
    mocks.vaultsGet.mockResolvedValue({
      vaults: [{ root: "/v", exclude: [], embed_exclude: [] }],
    });
    mocks.sync.mockRejectedValue("vault 폴더 이름이 겹칩니다: '노트'");
    await mount();
    await click(buttonByText("동기화"));

    expect(container.textContent).toContain("겹칩니다");
  });

  it("제거하면 목록에서 빠지고 저장된다", async () => {
    mocks.vaultsGet.mockResolvedValue({
      vaults: [{ root: "/v", exclude: [], embed_exclude: [] }],
    });
    await mount();
    expect(container.textContent).toContain("/v");

    await click(buttonByText("제거"));
    expect(mocks.vaultsSet).toHaveBeenCalledWith([]);
    expect(container.textContent).not.toContain("/v");
  });

  it("같은 폴더를 두 번 추가하지 않는다", async () => {
    // 중복은 UNIQUE 충돌이 아니라 무의미한 재스캔이다. 조용히 무시한다.
    mocks.vaultsGet.mockResolvedValue({
      vaults: [{ root: "/Users/me/Obsidian Vault", exclude: [], embed_exclude: [] }],
    });
    await mount();
    await click(buttonByText("폴더 추가"));
    await click(container.querySelector('[data-testid="pick"]'));

    expect(mocks.vaultsSet).not.toHaveBeenCalled();
  });

  it("동기화 중에는 버튼을 다시 누를 수 없다", async () => {
    // 색인은 수 분이 걸린다. 중복 실행하면 같은 vault를 두 번 훑는다.
    mocks.vaultsGet.mockResolvedValue({
      vaults: [{ root: "/v", exclude: [], embed_exclude: [] }],
    });
    let release: (v: unknown) => void = () => {};
    mocks.sync.mockReturnValue(new Promise((r) => (release = r)));
    await mount();

    await act(async () => {
      (buttonByText("동기화") as HTMLElement).click();
    });
    expect((buttonByText("동기화") as HTMLButtonElement).disabled).toBe(true);

    await act(async () => {
      release({ indexed: 0, skipped: 0, deleted: 0, edges: 0, embedded: 0 });
    });
  });
  it("문서 자료는 펼칠 때 처음 붙는다", async () => {
    // 색인 문서가 수천 건이다 — 설정을 여는 것만으로 그 목록을 읽게 두지 않는다.
    await mount();
    expect(container.textContent).not.toContain("문서 자료 패널");

    await click(buttonByText("둘러보기"));

    expect(container.textContent).toContain("문서 자료 패널 읽기만");

    await click(buttonByText("접기"));

    expect(container.textContent).not.toContain("문서 자료 패널");
  });
});
