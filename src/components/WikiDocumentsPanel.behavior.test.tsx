// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  connect: vi.fn(),
  documents: vi.fn(),
  read: vi.fn(),
  spaces: vi.fn(),
  sync: vi.fn(),
}));

vi.mock("../lib/wiki-ipc", () => ({
  wikiConnect: mocks.connect,
  wikiDocuments: mocks.documents,
  wikiReadDocument: mocks.read,
  wikiSpaces: mocks.spaces,
  wikiSync: mocks.sync,
}));
vi.mock("./ide/DirectoryPickerModal", () => ({
  DirectoryPickerModal: ({ onPick }: { onPick: (path: string) => void }) => (
    <button onClick={() => onPick("/notes")}>선택</button>
  ),
}));

import { WikiDocumentsPanel } from "./WikiDocumentsPanel";

const one = { node_id: 1, space_id: "one", relative_path: "work/one.md", title: "첫 문서", snippet: "alpha" };
const two = { node_id: 2, space_id: "one", relative_path: "two.md", title: "둘째 문서", snippet: "beta" };
let container: HTMLDivElement;
let root: Root;

const click = async (text: string) => {
  const button = [...container.querySelectorAll("button")].find(
    (item) => item.textContent === text || (text !== "선택" && item.textContent?.includes(text)),
  );
  expect(button).toBeTruthy();
  await act(async () => button?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
};

const query = async (value: string) => {
  const input = container.querySelector("input") as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
};

beforeEach(() => {
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  mocks.spaces.mockResolvedValue([{ id: "one", name: "노트", root: "/notes" }]);
  mocks.documents.mockResolvedValue({ documents: [one, two], truncated: false });
  mocks.connect.mockResolvedValue({ id: "one", name: "노트", root: "/notes" });
  mocks.sync.mockResolvedValue({ indexed: 2, skipped: 0, deleted: 0, edges: 0, complete: true, warnings: [] });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("WikiDocumentsPanel", () => {
  it("폴더 연결 성공 뒤 자동으로 색인한다", async () => {
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("폴더 연결");
    await click("선택");

    expect(mocks.connect).toHaveBeenCalledWith("/notes");
    expect(mocks.sync).toHaveBeenCalledTimes(1);
  });

  it("폴더 연결 실패 재시도는 같은 경로 연결을 다시 요청한다", async () => {
    mocks.connect.mockRejectedValueOnce(new Error("연결 실패"));
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("폴더 연결");
    await click("선택");
    await click("다시 시도");

    expect(mocks.connect).toHaveBeenCalledTimes(2);
    expect(mocks.connect).toHaveBeenLastCalledWith("/notes");
  });

  it("검색은 선택 그룹을 기본으로 하고 전체 전환을 명시한다", async () => {
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await query("alpha");
    expect(mocks.documents).toHaveBeenLastCalledWith("one", "alpha");

    await click("선택 그룹");
    await query("beta");
    expect(mocks.documents).toHaveBeenLastCalledWith(undefined, "beta");
  });

  it("전체 Wiki 전환은 선택한 하위 폴더 필터를 해제한다", async () => {
    mocks.spaces.mockResolvedValue([{ id: "one", name: "노트", root: "/notes" }, { id: "two", name: "공유", root: "/shared" }]);
    mocks.documents.mockResolvedValue({ documents: [one, { ...two, space_id: "two", relative_path: "outside.md" }], truncated: false });
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("work");
    expect(container.textContent).not.toContain("둘째 문서");

    await click("선택 그룹");
    expect(container.textContent).toContain("둘째 문서");

    await click("work");
    expect(container.textContent).not.toContain("둘째 문서");
  });

  it("색인 갱신 뒤 현재 검색을 다시 요청하고 미리보기를 비운다", async () => {
    mocks.read.mockResolvedValue({ ...one, path: "/notes/work/one.md", body: "preview" });
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("첫 문서");
    await query("alpha");
    const before = mocks.documents.mock.calls.filter(([, value]) => value === "alpha").length;
    await click("색인 갱신");

    expect(mocks.documents.mock.calls.filter(([, value]) => value === "alpha")).toHaveLength(before + 1);
    expect(container.textContent).not.toContain("preview");
  });

  it("새 문서 읽기가 실패하면 이전 미리보기를 남기지 않는다", async () => {
    mocks.read.mockResolvedValue({ ...two, path: "/notes/two.md", body: "fresh preview" }).mockResolvedValueOnce({ ...one, path: "/notes/work/one.md", body: "old preview" }).mockRejectedValueOnce(new Error("읽기 실패"));
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("첫 문서");
    await click("둘째 문서");

    expect(container.textContent).toContain("읽기 실패");
    expect(container.textContent).not.toContain("old preview");
    await click("다시 시도");
    expect(container.textContent).toContain("fresh preview");
  });

  it("첨부 실패는 같은 문서의 guarded attach만 재시도한다", async () => {
    const attach = vi.fn().mockRejectedValueOnce(new Error("첨부 실패")).mockResolvedValue(undefined);
    mocks.read.mockResolvedValue({ ...one, path: "/notes/work/one.md", body: "preview" });
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={attach} />));
    await click("첫 문서");
    await click("작업에 참조");
    await click("다시 시도");

    expect(attach).toHaveBeenCalledTimes(2);
    await click("work");
    expect(container.textContent).not.toContain("첨부 실패");
  });

  it("현재 검색이 실패하면 이전 문서를 결과인 것처럼 남기지 않는다", async () => {
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    mocks.documents.mockRejectedValueOnce(new Error("검색 실패"));
    await query("alpha");

    expect(container.textContent).toContain("검색 실패");
    expect(container.textContent).toContain("일치하는 문서가 없습니다.");
    expect(container.textContent).not.toContain("첫 문서");

    await click("다시 시도");
    expect(mocks.documents.mock.calls.filter(([, value]) => value === "alpha")).toHaveLength(2);
  });

  it("새 검색을 기다리는 동안 이전 결과를 비운다", async () => {
    let release: (value: unknown) => void = () => {};
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    mocks.documents.mockImplementation((_space, value) => value === "next" ? new Promise((resolve) => { release = resolve; }) : Promise.resolve({ documents: [one, two], truncated: false }));
    await query("next");

    expect(container.textContent).toContain("검색 중…");
    expect(container.textContent).not.toContain("첫 문서");
    await act(async () => release({ documents: [two], truncated: false }));
  });

  it("동기화 실패 재시도는 색인 작업을 다시 실행한다", async () => {
    mocks.sync.mockRejectedValueOnce(new Error("색인 실패"));
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("색인 갱신");
    await click("다시 시도");

    expect(mocks.sync).toHaveBeenCalledTimes(2);
  });

  it("metadata 실패 재시도는 metadata 요청을 다시 실행한다", async () => {
    mocks.spaces.mockRejectedValueOnce(new Error("목록 실패"));
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("다시 시도");

    expect(mocks.spaces).toHaveBeenCalledTimes(2);
  });

  it("느린 이전 metadata 응답이 연결 뒤 폴더 목록을 덮어쓰지 않는다", async () => {
    let oldSpaces: (value: unknown) => void = () => {};
    let oldDocuments: (value: unknown) => void = () => {};
    mocks.spaces.mockImplementationOnce(() => new Promise((resolve) => { oldSpaces = resolve; }));
    mocks.documents.mockImplementationOnce(() => new Promise((resolve) => { oldDocuments = resolve; }));
    mocks.connect.mockResolvedValue({ id: "two", name: "새 노트", root: "/new" });
    mocks.spaces.mockResolvedValue([{ id: "two", name: "새 노트", root: "/new" }]);
    mocks.documents.mockResolvedValue({ documents: [], truncated: false });
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("폴더 연결");
    await click("선택");
    await act(async () => {
      oldSpaces([{ id: "old", name: "이전 노트", root: "/old" }]);
      oldDocuments({ documents: [one], truncated: false });
    });

    expect(container.textContent).toContain("새 노트");
    expect(container.textContent).not.toContain("이전 노트");
  });

  it("늦게 끝난 문서 읽기가 새 선택을 덮어쓰지 않는다", async () => {
    let first: (value: unknown) => void = () => {};
    let second: (value: unknown) => void = () => {};
    mocks.read.mockImplementationOnce(() => new Promise((resolve) => { first = resolve; }));
    mocks.read.mockImplementationOnce(() => new Promise((resolve) => { second = resolve; }));
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("첫 문서");
    await click("둘째 문서");
    await act(async () => first({ ...one, path: "/notes/work/one.md", body: "first" }));
    await act(async () => second({ ...two, path: "/notes/two.md", body: "second" }));

    expect(container.textContent).toContain("second");
    expect(container.textContent).not.toContain("first");
  });

  it("긴 문서 목록은 처음에 제한된 행만 렌더하고 더 보기로 확장한다", async () => {
    mocks.documents.mockResolvedValue({
      documents: Array.from({ length: 201 }, (_, node_id) => ({ ...one, node_id, title: `문서 ${node_id}` })),
      truncated: false,
    });
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));

    expect(container.querySelectorAll('[aria-label="Wiki 문서"] button > span.text-sm.text-text')).toHaveLength(200);
    await click("더 보기");
    expect(container.querySelectorAll('[aria-label="Wiki 문서"] button > span.text-sm.text-text')).toHaveLength(201);
  });

  it("이미 연 문서도 다시 열면 디스크의 최신 내용을 읽는다", async () => {
    mocks.read.mockResolvedValue({ ...one, path: "/notes/work/one.md", body: "preview" });
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));

    await click("첫 문서");
    mocks.read.mockResolvedValue({ ...one, path: "/notes/work/one.md", body: "updated on disk" });
    await click("첫 문서");

    expect(mocks.read).toHaveBeenCalledTimes(2);
    expect(container.textContent).toContain("updated on disk");
  });

  it("색인 갱신 뒤에는 열린 문서를 다시 읽는다", async () => {
    mocks.read.mockResolvedValue({ ...one, path: "/notes/work/one.md", body: "preview" });
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("첫 문서");
    await click("색인 갱신");
    await click("첫 문서");

    expect(mocks.read).toHaveBeenCalledTimes(2);
  });

  it("문서 읽기 중에는 미리보기의 진행 상태를 보인다", async () => {
    mocks.read.mockImplementation(() => new Promise(() => {}));
    await act(async () => root.render(<WikiDocumentsPanel canAttach dark onAttach={vi.fn()} />));
    await click("첫 문서");

    expect(container.textContent).toContain("문서를 불러오는 중…");
  });
});
