// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConflictFile } from "../../lib/ipc";

const mocks = vi.hoisted(() => ({
  begin: vi.fn(),
  resolve: vi.fn(),
  finish: vi.fn(),
  abort: vi.fn(),
}));

// `summarizePatch`·`toSplitRows`는 실제 구현을 쓴다 — diff 해석을 테스트가 다시 구현하면
// 복제본을 검증하는 셈이라 회귀를 잡지 못한다.
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/ipc")>()),
  conflictBegin: mocks.begin,
  conflictResolve: mocks.resolve,
  conflictFinish: mocks.finish,
  conflictAbort: mocks.abort,
}));

import { ConflictResolver } from "./ConflictResolver";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** 실질 차이가 마지막 한 줄뿐인 충돌 — 생성물 문서에서 실제로 나오는 모양이다. */
const NEARLY_IDENTICAL: ConflictFile = {
  path: "docs/TODO.md",
  ours: "# TODO\n\n총 1건\n<!-- state: 111 -->\n",
  theirs: "# TODO\n\n총 1건\n<!-- state: 222 -->\n",
  base: "# TODO\n\n총 1건\n<!-- state: 000 -->\n",
  patch: [
    "@@ -1,4 +1,4 @@",
    " # TODO",
    " ",
    " 총 1건",
    "-<!-- state: 111 -->",
    "+<!-- state: 222 -->",
    "",
  ].join("\n"),
};

/** base 쪽이 파일을 지운 충돌 — 비교 상대가 없어 patch가 만들어지지 않는다. */
const DELETED_ON_BASE: ConflictFile = {
  path: "src/gone.ts",
  ours: "export const kept = 1;\n",
  theirs: null,
  base: "export const kept = 0;\n",
  patch: null,
};

let container: HTMLDivElement;
let root: Root;

async function render(files: ConflictFile[]) {
  mocks.begin.mockResolvedValue(files);
  await act(async () => {
    root.render(<ConflictResolver taskId={1} onClose={() => {}} />);
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("ConflictResolver", () => {
  it("갈린 줄에만 색을 입혀 짚는다 — 찾는 일을 사람에게 넘기지 않는다", async () => {
    await render([NEARLY_IDENTICAL]);
    const cells = [...container.querySelectorAll("div")];
    const toned = (tone: string) => cells.filter((cell) => cell.className.includes(tone));

    // 갈린 두 줄이 각각 삭제/추가 톤으로 표시된다.
    const removed = toned("bg-delbg");
    const added = toned("bg-addbg");
    expect(removed.map((c) => c.textContent).join()).toContain("<!-- state: 111 -->");
    expect(added.map((c) => c.textContent).join()).toContain("<!-- state: 222 -->");

    // 그리고 그게 전부다 — 같은 줄까지 칠하면 강조가 아무 의미도 갖지 못한다.
    expect(removed).toHaveLength(1);
    expect(added).toHaveLength(1);
    expect([...removed, ...added].map((c) => c.textContent).join()).not.toContain("총 1건");
  });

  it("차이의 규모를 세어 보여준다 — 열어보기 전에 판단이 서야 한다", async () => {
    await render([NEARLY_IDENTICAL]);
    const badge = container.querySelector("[title*='에만 있는 줄']");
    expect(badge?.textContent).toContain("−1");
    expect(badge?.textContent).toContain("+1");
  });

  it("한쪽이 삭제된 충돌은 비교 대신 전문을 그대로 놓는다", async () => {
    await render([DELETED_ON_BASE]);
    const text = container.textContent ?? "";
    expect(text).toContain("export const kept = 1;");
    expect(text).toContain("(이 쪽에는 파일이 없습니다)");
  });

  it("긴 한 줄이 가로 스크롤에 잘리지 않는다", async () => {
    await render([DELETED_ON_BASE]);
    const pane = container.querySelector("pre");
    expect(pane?.className).toContain("whitespace-pre-wrap");
    expect(pane?.className).toContain("break-all");
  });
});
