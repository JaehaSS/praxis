// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const mocks = vi.hoisted(() => ({ documents: vi.fn(), detail: vi.fn(), scan: vi.fn(), search: vi.fn(), session: vi.fn(), status: vi.fn(), listen: vi.fn() }));
vi.mock("../../lib/knowledge-vault-ipc", () => ({ vaultArchive: vi.fn(), vaultDocument: mocks.detail, vaultDocuments: mocks.documents, vaultScan: mocks.scan, vaultSearch: mocks.search, vaultSessionOpen: mocks.session, vaultStatus: mocks.status }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("./VaultAddSource", () => ({ VaultAddSource: ({ onSaved }: { onSaved: () => Promise<void> }) => <button type="button" onClick={() => void onSaved()}>가져오기 완료</button> }));
vi.mock("./VaultDocumentDetail", () => ({ VaultDocumentDetail: ({ detail, onDirtyChange }: { detail: { document: { title: string } }; onDirtyChange?: (dirty: boolean) => void }) => <section><p>상세 {detail.document.title}</p><button type="button" onClick={() => onDirtyChange?.(true)}>노트 편집 중</button></section> }));
vi.mock("./VaultSettings", () => ({ VaultSettings: () => null }));
vi.mock("./VaultFolderSettings", () => ({ VaultFolderSettings: () => <p>창고 폴더 설정</p> }));
vi.mock("./VaultMemoryPanel", () => ({ VaultMemoryPanel: ({ agent }: { agent?: string }) => <p>메모리 패널 {agent ?? "claude"}</p> }));
vi.mock("./WikiWorkspace", () => ({ WikiWorkspace: ({ onDirtyChange }: { onDirtyChange?: (dirty: boolean) => void }) => <section aria-label="파일 위키"><h2>위키 문서</h2><p>하네스 문서 목록</p><button type="button" onClick={() => onDirtyChange?.(true)}>문서 편집 중</button></section> }));
import { VaultView } from "./VaultView";

let node: HTMLDivElement; let root: Root; let editorClosed: (event: { payload: string }) => void;
beforeEach(() => { mocks.status.mockReset(); mocks.documents.mockReset(); mocks.detail.mockReset(); mocks.scan.mockReset(); mocks.search.mockReset(); mocks.session.mockReset(); mocks.listen.mockReset(); node = document.createElement("div"); document.body.appendChild(node); root = createRoot(node); mocks.status.mockResolvedValue({ supported: true, vaults: [{ id: "v", vault_root: "/vault", enabled: true }] }); mocks.scan.mockResolvedValue({ indexed: 0, skipped: 0, partial: false, warnings: [] }); mocks.session.mockResolvedValue({ label: "vault", root: "/vault", skill: "wiki-organizer", skill_present: true, warning: null }); mocks.listen.mockImplementation((_event: string, handler: (event: { payload: string }) => void) => { editorClosed = handler; return Promise.resolve(() => {}); }); });
afterEach(() => { act(() => root.unmount()); node.remove(); });

it("opens wiki documents by default", async () => {
  mocks.documents.mockResolvedValue([note, source]);
  await act(async () => { root.render(<VaultView host="local" />); await Promise.resolve(); await Promise.resolve(); });
  expect(node.textContent).toContain("위키 문서"); expect(node.textContent).toContain("하네스 문서 목록"); expect(node.textContent).not.toContain("원자료");
});

it("does not call local IPC for a remote host", async () => {
  await act(async () => { root.render(<VaultView host="remote" />); });
  expect(node.textContent).toContain("로컬 세션에서만"); expect(mocks.status).not.toHaveBeenCalled();
});

it("does not load documents when unsupported", async () => {
  mocks.status.mockResolvedValue({ supported: false, vaults: [] });
  await act(async () => { root.render(<VaultView host="local" />); await Promise.resolve(); });
  expect(node.textContent).toContain("macOS Desktop"); expect(mocks.documents).not.toHaveBeenCalled();
});

it("pages matching search results after one hundred rows", async () => {
  const documents = Array.from({ length: 101 }, (_, index) => ({ ...source, id: `d${index}`, title: `자료 ${index}` }));
  mocks.documents.mockResolvedValue(documents);
  mocks.search
    .mockResolvedValueOnce({ hits: documents.slice(0, 100).map(item => ({ document_id: item.id })), has_more: true })
    .mockResolvedValueOnce({ hits: [{ document_id: "d100" }], has_more: false });

  await renderLoaded();
  await act(async () => { button("자료").click(); });
  await act(async () => { input(node.querySelector("input[aria-label='자료 검색']") as HTMLInputElement, "자료"); await Promise.resolve(); });

  expect(node.textContent).not.toContain("자료 100");

  await act(async () => { button("다음").click(); await Promise.resolve(); });

  expect(node.textContent).toContain("자료 100");
  expect(mocks.search).toHaveBeenLastCalledWith("자료", 100, "local");
});

it("ignores a stale search response after the query changes", async () => {
  const documents = ["old", "new"].map(id => ({ ...source, id, title: id }));
  const pending: Array<(value: { hits: Array<{ document_id: string }>; has_more: boolean }) => void> = [];
  mocks.documents.mockResolvedValue(documents);
  mocks.search.mockImplementation(() => new Promise(resolve => pending.push(resolve)));

  await renderLoaded();
  await act(async () => { button("자료").click(); });
  const search = node.querySelector("input[aria-label='자료 검색']") as HTMLInputElement;
  await act(async () => { input(search, "old"); await Promise.resolve(); });
  await act(async () => { input(search, "new"); await Promise.resolve(); });
  await act(async () => { pending[1]({ hits: [{ document_id: "new" }], has_more: false }); await Promise.resolve(); pending[0]({ hits: [{ document_id: "old" }], has_more: false }); await Promise.resolve(); });

  expect(node.textContent).toContain("new");
  expect(node.textContent).not.toContain("old");
});

it("refreshes an unchanged search after importing a matching source", async () => {
  const imported = { ...source, id: "new", title: "새 연구 자료" };
  mocks.documents.mockResolvedValue([]);
  mocks.search.mockResolvedValueOnce({ hits: [], has_more: false }).mockResolvedValueOnce({ hits: [{ document_id: "new" }], has_more: false });

  await renderLoaded();
  await act(async () => { button("자료").click(); });
  await act(async () => { input(node.querySelector("input[aria-label='자료 검색']") as HTMLInputElement, "연구"); await Promise.resolve(); });
  expect(node.textContent).toContain("일치하는 자료가 없습니다.");

  mocks.documents.mockResolvedValue([imported]);
  await act(async () => { button("가져오기 완료").click(); await Promise.resolve(); await Promise.resolve(); });

  expect(node.textContent).toContain("새 연구 자료");
  expect(mocks.search).toHaveBeenLastCalledWith("연구", 0, "local");
});

it("keeps a dirty file editor mounted when navigation and header actions are attempted", async () => {
  mocks.documents.mockResolvedValue([note, source]);
  await renderLoaded();
  await act(async () => { button("문서 편집 중").click(); });
  await act(async () => { button("자료").click(); button("가져오기 완료").click(); await flush(); });
  expect(node.querySelector("[aria-label='파일 위키']")).not.toBeNull();
  expect(node.textContent).toContain("수정 또는 저장 중인 내용이 있습니다.");
  expect(mocks.scan).toHaveBeenCalledTimes(1);
});

it("scans once on mount and again when the vault editor window closes", async () => {
  mocks.documents.mockResolvedValue([note]);

  await renderLoaded();
  expect(mocks.scan).toHaveBeenCalledTimes(1);
  expect(mocks.scan).toHaveBeenCalledWith("v", [], "private-data", undefined, "local");

  await act(async () => { editorClosed({ payload: "/other" }); await flush(); });
  expect(mocks.scan).toHaveBeenCalledTimes(1);

  await act(async () => { editorClosed({ payload: "/vault" }); await flush(); });
  expect(mocks.scan).toHaveBeenCalledTimes(2);
});

it("offers exactly three tabs and no proposal entry", async () => {
  mocks.documents.mockResolvedValue([note]);

  await renderLoaded();
  // 문서 · 자료 · 메모리 — 제안 검토는 계획 2026-09-13에서 채널 밖으로 나갔다.
  expect(Array.from(node.querySelectorAll("button[aria-pressed]"), item => item.textContent)).toEqual(["문서", "자료", "메모리"]);
  expect(node.textContent).not.toContain("검토할 초안");
  expect(node.textContent).not.toContain("위키 초안 만들기");
  expect(node.querySelector("input[type=checkbox]")).toBeNull();
});

it("shows the folder settings form inside the management panel", async () => {
  mocks.documents.mockResolvedValue([note]);

  await renderLoaded();
  expect(node.textContent).not.toContain("창고 폴더 설정");

  await act(async () => { button("관리").click(); });

  expect(node.textContent).toContain("창고 폴더 설정");
  expect(node.textContent).not.toContain("기존 Wiki 열기");
});

it("shows memory without a connected vault", async () => {
  // 메모리 파일은 창고 연결과 무관하다 — "연결된 자료함이 없습니다"에 갇히면 안 된다(설계 R7).
  mocks.status.mockResolvedValue({ supported: true, vaults: [] });
  await act(async () => { root.render(<VaultView host="local" />); });
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  expect(node.textContent).toContain("연결된 자료함이 없습니다");

  await act(async () => { button("메모리").click(); });

  expect(node.textContent).toContain("메모리 패널");
  expect(node.textContent).not.toContain("연결된 자료함이 없습니다");
  expect(node.querySelector("input[aria-label='자료 검색']")).toBeNull();
});

it("names the configured skill when an organizing session cannot find it", async () => {
  // 스킬 이름은 설정값이다 — 안내가 기본값을 하드코딩하면 knowledge-harness를 쓰는 사용자가 엉뚱한 경로를 본다.
  mocks.documents.mockResolvedValue([note]);
  mocks.session.mockResolvedValue({ label: "vault", root: "/vault", skill: "knowledge-harness", skill_present: false, warning: null });

  await renderLoaded();
  await act(async () => { button("정리 세션 열기").click(); await flush(); });

  expect(mocks.session).toHaveBeenCalledWith("v", "claude", "local");
  expect(node.textContent).toContain("~/.claude/skills/knowledge-harness 스킬이 없습니다");
  expect(node.textContent).not.toContain("wiki-organizer");
});

it("shows a rejected organizing session as an error", async () => {
  mocks.documents.mockResolvedValue([note]);
  mocks.session.mockRejectedValue("에이전트 CLI를 찾지 못했습니다.");

  await renderLoaded();
  await act(async () => { button("정리 세션 열기").click(); await flush(); });

  expect(node.querySelector("[role=alert]")?.textContent).toContain("에이전트 CLI를 찾지 못했습니다.");
});

it("has no organizing session button on a remote host", async () => {
  await act(async () => { root.render(<VaultView host="remote" />); });
  expect(Array.from(node.querySelectorAll("button")).some(item => item.textContent === "정리 세션 열기")).toBe(false);
});

const note = { id: "note", vault_id: "v", kind: "note" as const, title: "주제 문서", state: "active" as const, current_revision_id: "r2", current_scope: "private-data" as const };
const source = { id: "source", vault_id: "v", kind: "source" as const, title: "원자료", state: "active" as const, current_revision_id: "r1", current_scope: "private-data" as const };
function button(text: string) { return Array.from(node.querySelectorAll<HTMLButtonElement>("button")).find(item => item.textContent === text)!; }
function input(element: HTMLInputElement, value: string) { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(element, value); element.dispatchEvent(new Event("input", { bubbles: true })); }
async function renderLoaded() { await act(async () => { root.render(<VaultView host="local" />); await flush(); await flush(); }); }
async function flush() { await Promise.resolve(); await Promise.resolve(); }
