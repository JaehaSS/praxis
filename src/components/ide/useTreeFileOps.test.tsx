// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FsNode } from "../../lib/ipc";

const mocks = vi.hoisted(() => ({
  fsCreateFile: vi.fn(async (dir: string, name: string) => `/w/${dir}/${name}`),
  fsCreateDir: vi.fn(async (dir: string, name: string) => `/w/${dir}/${name}`),
  // 워크트리 루트가 `/w`인 셈 — 상대 경로를 그대로 이어 붙인다.
  resolveAbsPath: vi.fn(async (_id: number, path: string) => (path === "" ? "/w" : `/w/${path}`)),
  fsRename: vi.fn(async (path: string, name: string) => `${path}/../${name}`),
  fsTrash: vi.fn(async () => undefined),
}));

vi.mock("../../lib/ipc", () => ({
  fsCreateFile: mocks.fsCreateFile,
  fsCreateDir: mocks.fsCreateDir,
  resolveAbsPath: mocks.resolveAbsPath,
  fsRename: mocks.fsRename,
  fsTrash: mocks.fsTrash,
}));

import { useTreeFileOps } from "./useTreeFileOps";
import { diffTabKey, fileTabKey } from "../../lib/tab-key";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const dir = (name: string, path: string, children: FsNode[] = []): FsNode => ({
  name,
  path,
  is_dir: true,
  children,
});
const file = (name: string, path: string): FsNode => ({ name, path, is_dir: false, children: [] });

const TREE: FsNode[] = [
  dir("src", "src", [file("App.tsx", "src/App.tsx")]),
  file("README.md", "README.md"),
];

let api: ReturnType<typeof useTreeFileOps> | null = null;
const refreshTree = vi.fn();
const openFile = vi.fn(async (_path: string, _opts?: { preview?: boolean }) => true);
const closeTabsForPath = vi.fn();
const onError = vi.fn();

function Harness({
  canMutate = true,
  openFiles = [],
  activePath = null,
}: {
  canMutate?: boolean;
  openFiles?: Array<{ path: string; dirty: boolean; diff?: boolean }>;
  activePath?: string | null;
}) {
  api = useTreeFileOps({
    taskId: 42,
    tree: TREE,
    refreshTree,
    openFile,
    canMutate,
    openFiles: openFiles.map((f) => ({
      key: f.diff === true ? diffTabKey(f.path) : fileTabKey(f.path),
      path: f.path,
      dirty: f.dirty,
    })),
    activePath,
    closeTabsForPath,
    onError,
  });
  return null;
}

let container: HTMLDivElement;
let root: Root;

const render = async (props: Parameters<typeof Harness>[0] = {}) => {
  await act(async () => {
    root.render(<Harness {...props} />);
    await Promise.resolve();
  });
};

/** 메뉴를 열고 그 항목을 고른다 — 실제 클릭 경로와 같은 순서다. */
const pick = async (node: FsNode | null, label: string) => {
  await act(async () => api?.openMenu(node, 0, 0));
  const row = api?.rows.find((r) => r?.label === label);
  expect(row, `${label} 항목이 없다`).toBeTruthy();
  await act(async () => row?.onSelect());
};

const confirm = async (name: string) => {
  await act(async () => {
    api?.promptProps.onConfirm(name);
    await Promise.resolve();
  });
  // 체인이 두 단계(resolveAbsPath → create)라 마이크로태스크를 한 번 더 흘린다.
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
};

beforeEach(() => {
  vi.clearAllMocks();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  api = null;
});

describe("새 파일·폴더", () => {
  it("디렉터리를 짚었으면 그 안에 만든다", async () => {
    await render();
    await pick(TREE[0], "새 파일…");
    expect(api?.promptProps.subtitle).toBe("src");

    await confirm("새.ts");

    expect(mocks.resolveAbsPath).toHaveBeenCalledWith(42, "src");
    expect(mocks.fsCreateFile).toHaveBeenCalledWith("/w/src", "새.ts");
  });

  it("파일을 짚었으면 그 옆에 만든다", async () => {
    await render();
    await pick(TREE[0].children[0], "새 파일…");

    await confirm("이웃.ts");

    expect(mocks.fsCreateFile).toHaveBeenCalledWith("/w/src", "이웃.ts");
  });

  it("빈 자리를 짚었으면 워크트리 루트에 만든다", async () => {
    await render();
    await pick(null, "새 파일…");
    expect(api?.promptProps.subtitle).toBe("(워크트리 루트)");

    await confirm("루트.md");

    expect(mocks.resolveAbsPath).toHaveBeenCalledWith(42, "");
    expect(mocks.fsCreateFile).toHaveBeenCalledWith("/w", "루트.md");
  });

  it("만든 파일은 곧바로 연다 — 트리에서 다시 찾게 하지 않는다", async () => {
    await render();
    await pick(TREE[0], "새 파일…");
    await confirm("새.ts");

    expect(refreshTree).toHaveBeenCalled();
    // 탭이 쓰는 형식은 상대 경로다(백엔드가 돌려주는 절대 경로가 아니라).
    expect(openFile).toHaveBeenCalledWith("src/새.ts");
  });

  it("폴더는 만들기만 하고 열지 않는다", async () => {
    await render();
    await pick(TREE[0], "새 폴더…");
    await confirm("hooks");

    expect(mocks.fsCreateDir).toHaveBeenCalledWith("/w/src", "hooks");
    expect(openFile).not.toHaveBeenCalled();
    expect(refreshTree).toHaveBeenCalled();
  });

  it("그 디렉터리의 이름들을 넘겨 중복을 제출 전에 잡는다", async () => {
    await render();
    await pick(TREE[0], "새 파일…");

    expect([...(api?.promptProps.taken ?? [])]).toEqual(["App.tsx"]);
  });

  it("백엔드가 거부하면 다이얼로그를 닫지 않고 사유를 보여 준다", async () => {
    mocks.fsCreateFile.mockRejectedValueOnce(new Error("이미 존재합니다"));
    await render();
    await pick(TREE[0], "새 파일…");
    await confirm("App.tsx");

    expect(api?.promptProps.open).toBe(true);
    expect(api?.promptProps.serverError).toContain("이미 존재합니다");
    expect(refreshTree).not.toHaveBeenCalled();
  });

  it("취소하면 사유도 함께 지운다", async () => {
    mocks.fsCreateFile.mockRejectedValueOnce(new Error("거부"));
    await render();
    await pick(TREE[0], "새 파일…");
    await confirm("x.ts");
    await act(async () => api?.promptProps.onCancel());

    expect(api?.promptProps.open).toBe(false);
    expect(api?.promptProps.serverError).toBeNull();
  });

  it("원격 워크트리에서는 생성 항목이 눌리지 않는다", async () => {
    // 생성 IPC는 로컬 고정이라, 눌러 본 뒤 실패를 보는 것보다 눌리지 않는 편이 낫다.
    await render({ canMutate: false });
    await act(async () => api?.openMenu(TREE[0], 0, 0));

    const rows = api?.rows.filter((r) => r?.key === "newFile" || r?.key === "newDir") ?? [];
    expect(rows).toHaveLength(2);
    expect(rows.every((r) => r?.disabled === true)).toBe(true);
  });

  it("빈 자리 메뉴에는 그 노드에 대한 항목을 두지 않는다", async () => {
    await render();
    await act(async () => api?.openMenu(null, 0, 0));

    const keys = (api?.rows ?? []).flatMap((r) => (r ? [r.key] : []));
    expect(keys).toEqual(["newFile", "newDir"]);
  });
});

describe("이름 변경", () => {
  const OPEN = [
    { path: "src/App.tsx", dirty: false },
    { path: "README.md", dirty: false },
  ];

  it("현재 이름으로 시작하고, 자기 이름은 중복으로 치지 않는다", async () => {
    await render();
    await pick(TREE[0].children[0], "이름 변경…");

    expect(api?.promptProps.title).toBe("이름 변경");
    expect(api?.promptProps.initial).toBe("App.tsx");
    expect(api?.promptProps.subtitle).toBe("src");
    // 자기 이름이 taken에 남으면 다이얼로그가 열리자마자 빨간불이 켜진다.
    expect([...(api?.promptProps.taken ?? [])]).toEqual([]);
  });

  it("열려 있던 탭을 새 경로로 다시 연다", async () => {
    await render({ openFiles: OPEN, activePath: "src/App.tsx" });
    await pick(TREE[0].children[0], "이름 변경…");
    await confirm("Root.tsx");

    expect(mocks.fsRename).toHaveBeenCalledWith("/w/src/App.tsx", "Root.tsx");
    expect(closeTabsForPath).toHaveBeenCalledWith("src/App.tsx");
    expect(openFile).toHaveBeenCalledWith("src/Root.tsx");
    // 무관한 탭은 건드리지 않는다.
    expect(closeTabsForPath).not.toHaveBeenCalledWith("README.md");
  });

  it("폴더를 바꾸면 그 아래 탭들이 함께 따라간다", async () => {
    await render({
      openFiles: [{ path: "src/App.tsx", dirty: false }, { path: "README.md", dirty: false }],
      activePath: "src/App.tsx",
    });
    await pick(TREE[0], "이름 변경…");
    await confirm("app");

    expect(mocks.fsRename).toHaveBeenCalledWith("/w/src", "app");
    expect(closeTabsForPath).toHaveBeenCalledWith("src/App.tsx");
    expect(openFile).toHaveBeenCalledWith("app/App.tsx");
  });

  it("원래 보던 탭을 맨 마지막에 열어 앞으로 되돌린다", async () => {
    await render({
      openFiles: [
        { path: "src/App.tsx", dirty: false },
        { path: "src/lib.ts", dirty: false },
      ],
      activePath: "src/App.tsx",
    });
    await pick(TREE[0], "이름 변경…");
    await confirm("app");

    const opened = openFile.mock.calls.map((c) => c[0]);
    expect(opened).toContain("app/lib.ts");
    expect(opened[opened.length - 1]).toBe("app/App.tsx");
  });

  it("diff 탭은 함께 닫히되 파일로 되살아나지 않는다", async () => {
    await render({
      openFiles: [
        { path: "src/App.tsx", dirty: false },
        { path: "src/lib.ts", dirty: false, diff: true },
      ],
      activePath: "src/App.tsx",
    });
    await pick(TREE[0], "이름 변경…");
    await confirm("app");

    // 사라진 경로의 diff 탭도 닫는다 — 남기면 없는 파일의 변경분을 그리게 된다.
    expect(closeTabsForPath).toHaveBeenCalledWith("src/lib.ts");
    expect(openFile.mock.calls.map((c) => c[0])).toEqual(["app/App.tsx"]);
  });

  it("저장하지 않은 편집이 걸려 있으면 시작하지도 않는다", async () => {
    await render({ openFiles: [{ path: "src/App.tsx", dirty: true }] });
    await pick(TREE[0], "이름 변경…");

    // 휴지통은 파일을 돌려주지만 저장 안 한 내용은 아무도 돌려주지 않는다.
    expect(api?.promptProps.open).toBe(false);
    expect(onError).toHaveBeenCalledWith(expect.stringContaining("src/App.tsx"));
    expect(mocks.fsRename).not.toHaveBeenCalled();
  });

  it("백엔드가 거부하면 다이얼로그를 닫지 않는다", async () => {
    mocks.fsRename.mockRejectedValueOnce(new Error("같은 이름이 있습니다"));
    await render();
    await pick(TREE[0].children[0], "이름 변경…");
    await confirm("README.md");

    expect(api?.promptProps.open).toBe(true);
    expect(api?.promptProps.serverError).toContain("같은 이름이 있습니다");
    expect(closeTabsForPath).not.toHaveBeenCalled();
  });
});

describe("휴지통으로 이동", () => {
  it("지운 파일의 탭을 닫는다", async () => {
    await render({ openFiles: [{ path: "src/App.tsx", dirty: false }] });
    await pick(TREE[0].children[0], "휴지통으로 이동");

    expect(mocks.fsTrash).toHaveBeenCalledWith("/w/src/App.tsx");
    expect(closeTabsForPath).toHaveBeenCalledWith("src/App.tsx");
    expect(refreshTree).toHaveBeenCalled();
  });

  it("폴더를 지우면 그 아래 탭이 전부 닫힌다", async () => {
    await render({
      openFiles: [
        { path: "src/App.tsx", dirty: false },
        { path: "README.md", dirty: false },
      ],
    });
    await pick(TREE[0], "휴지통으로 이동");

    expect(closeTabsForPath).toHaveBeenCalledWith("src/App.tsx");
    expect(closeTabsForPath).not.toHaveBeenCalledWith("README.md");
  });

  it("저장하지 않은 편집이 걸려 있으면 지우지 않는다", async () => {
    await render({ openFiles: [{ path: "src/App.tsx", dirty: true }] });
    await pick(TREE[0].children[0], "휴지통으로 이동");

    expect(mocks.fsTrash).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(expect.stringContaining("src/App.tsx"));
  });

  it("실패는 배너로 알린다 — 메뉴는 이미 닫혔다", async () => {
    mocks.fsTrash.mockRejectedValueOnce(new Error("권한이 없습니다"));
    await render();
    await pick(TREE[0].children[0], "휴지통으로 이동");

    expect(onError).toHaveBeenCalledWith(expect.stringContaining("권한이 없습니다"));
    expect(refreshTree).not.toHaveBeenCalled();
  });

  it("원격 워크트리에서는 둘 다 눌리지 않는다", async () => {
    await render({ canMutate: false });
    await act(async () => api?.openMenu(TREE[0], 0, 0));

    const rows = api?.rows.filter((r) => r?.key === "rename" || r?.key === "trash") ?? [];
    expect(rows).toHaveLength(2);
    expect(rows.every((r) => r?.disabled === true)).toBe(true);
  });
});

