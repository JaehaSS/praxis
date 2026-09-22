// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { VaultDocument, VaultDocumentDetail as Detail } from "../../lib/knowledge-vault-ipc";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const mocks = vi.hoisted(() => ({ open: vi.fn(), detail: vi.fn(), update: vi.fn() }));
vi.mock("../../lib/knowledge-vault-ipc", () => ({ vaultOpenOriginal: mocks.open, vaultScope: vi.fn(), vaultCreateNote: vi.fn(), vaultDocument: mocks.detail, vaultUpdateNote: mocks.update }));
import { VaultDocumentDetail } from "./VaultDocumentDetail";
it("opens the original only after its button is clicked", async () => { const node = document.createElement("div"); const root = createRoot(node); const detail: any = { document: { id: "d", vault_id: "v", title: "t", current_scope: "private-data", state: "active" }, current_revision: { id: "r" }, revision_history: [], source_revisions: [], backlinks: [], bounded_text: null, index_status: "indexed", index_reason: null }; await act(async () => { root.render(<VaultDocumentDetail detail={detail} host="local" onArchive={() => {}} onChanged={async () => {}} />); }); expect(mocks.open).not.toHaveBeenCalled(); await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "원본 열기") as HTMLButtonElement).click(); }); expect(mocks.open).toHaveBeenCalledWith("r", "local"); root.unmount(); });

it("updates a note with the displayed revision as its expected base", async () => { mocks.update.mockResolvedValue({ id: "next" }); const node = document.createElement("div"); const root = createRoot(node); const detail: any = { document: { id: "d", vault_id: "v", kind: "note", title: "t", current_scope: "private-data", state: "active" }, current_revision: { id: "r" }, revision_history: [], source_revisions: [], backlinks: [], bounded_text: "editable body", index_status: "indexed", index_reason: null }; await act(async () => { root.render(<VaultDocumentDetail detail={detail} host="local" onArchive={() => {}} onChanged={async () => {}} />); }); await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "노트 수정") as HTMLButtonElement).click(); }); await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "수정 저장") as HTMLButtonElement).click(); }); expect(mocks.update).toHaveBeenCalledWith("v", "d", "r", "editable body", [], "private-data", undefined, "local"); root.unmount(); });

it("navigates through a related document while opening its exact source revision", async () => { const navigate = vi.fn(); const node = document.createElement("div"); const root = createRoot(node); const detail: any = { document: { id: "d", vault_id: "v", title: "t", current_scope: "private-data", state: "active" }, current_revision: { id: "r" }, revision_history: [], source_revisions: [{ document_id: "source-doc", title: "source title", revision_id: "old-source" }], backlinks: [], bounded_text: null, index_status: "indexed", index_reason: null }; await act(async () => { root.render(<VaultDocumentDetail detail={detail} host="local" onArchive={() => {}} onChanged={async () => {}} onNavigate={navigate} />); }); await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "source title") as HTMLButtonElement).click(); }); expect(navigate).toHaveBeenCalledWith("source-doc"); await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "출처 버전 열기") as HTMLButtonElement).click(); }); expect(mocks.open).toHaveBeenCalledWith("old-source", "local"); root.unmount(); });

it("links a source into a loaded existing note using its current base", async () => { mocks.detail.mockResolvedValue({ document: { id: "note", vault_id: "v", kind: "note", title: "note", current_scope: "private-data", state: "active" }, current_revision: { id: "base", sha256: "hash" }, revision_history: [], source_revisions: [], backlinks: [], bounded_text: "before", index_status: "indexed", index_reason: null }); mocks.update.mockResolvedValue({ id: "next" }); const node = document.createElement("div"); const root = createRoot(node); const detail: any = { document: { id: "source", vault_id: "v", kind: "source", title: "source", current_scope: "private-data", state: "active" }, current_revision: { id: "source-revision" }, revision_history: [], source_revisions: [], backlinks: [], bounded_text: null, index_status: "indexed", index_reason: null }; const documents: any[] = [{ id: "note", vault_id: "v", kind: "note", title: "note", current_scope: "private-data", state: "active" }]; await act(async () => { root.render(<VaultDocumentDetail detail={detail} documents={documents} host="local" onArchive={() => {}} onChanged={async () => {}} />); }); await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "노트 만들기") as HTMLButtonElement).click(); }); const target = node.querySelector("select[aria-label='연결할 기존 노트']") as HTMLSelectElement; await act(async () => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(target, "note"); target.dispatchEvent(new Event("change", { bubbles: true })); }); expect(node.textContent).toContain("hash"); await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "기존 노트에 저장") as HTMLButtonElement).click(); }); expect(mocks.update).toHaveBeenCalledWith("v", "note", "base", "before", ["source-revision"], "private-data", undefined, "local"); root.unmount(); });

it("keeps a direct note draft through a cancel that the user takes back", async () => {
  mocks.open.mockReset(); mocks.update.mockReset();
  const changed = vi.fn().mockResolvedValue(undefined);
  const node = document.createElement("div"); const root = createRoot(node);
  try {
    await act(async () => { root.render(<VaultDocumentDetail detail={noteDetail()} host="local" onArchive={() => {}} onChanged={changed} />); });
    await act(async () => { find(node, "노트 수정").click(); });
    await act(async () => { setText(node.querySelector("textarea")!, "draft"); });

    // 편집 중에는 원본 열기·범위 저장이 잠긴다 — 파일이 밖에서 바뀌면 기준이 흔들린다.
    expect(find(node, "원본 열기").disabled).toBe(true);

    await act(async () => { find(node, "취소").click(); });
    await act(async () => { find(node, "계속 편집").click(); });

    expect(changed).not.toHaveBeenCalled();
    expect((node.querySelector("textarea") as HTMLTextAreaElement).value).toBe("draft");
    expect(find(node, "노트 수정").disabled).toBe(true);
  } finally { await act(async () => { root.unmount(); }); }
});

it("shows neither drift acceptance nor a revision history", async () => {
  // 정본은 파일이다 — 리비전은 git이 하고, 바뀐 내용은 다시 스캔하면 잡힌다(계획 2026-09-13).
  const node = document.createElement("div"); const root = createRoot(node);
  const detail = noteDetail({ revision_history: [{ id: "history", document_id: "note", relative_path: "notes/n.md", sha256: "history-hash", size: 1, predecessor: null }] });
  try {
    await act(async () => { root.render(<VaultDocumentDetail detail={detail} host="local" onArchive={() => {}} onChanged={async () => {}} />); });

    expect(node.textContent).not.toContain("변경 내용 확인");
    expect(node.textContent).not.toContain("버전 이력");
    expect(node.textContent).not.toContain("이 버전 원본 열기");
  } finally { await act(async () => { root.unmount(); }); }
});

it("disables saving a direct note when its edited body is blank", async () => {
  mocks.update.mockReset();
  const node = document.createElement("div"); const root = createRoot(node);
  try {
    await act(async () => { root.render(<VaultDocumentDetail detail={noteDetail()} host="local" onArchive={() => {}} onChanged={async () => {}} />); });
    await act(async () => { find(node, "노트 수정").click(); });
    await act(async () => { setText(node.querySelector("textarea")!, "   "); });

    expect(find(node, "수정 저장").disabled).toBe(true);
    expect(mocks.update).not.toHaveBeenCalled();
  } finally { await act(async () => { root.unmount(); }); }
});

it("does not replace a dirty source composition with an existing target", async () => {
  mocks.detail.mockReset();
  const node = document.createElement("div"); const root = createRoot(node);
  const sourceDetail: Detail = { ...noteDetail(), document: { id: "source", vault_id: "v", kind: "source", title: "source", current_scope: "private-data", state: "active", current_revision_id: "source-revision" }, current_revision: { id: "source-revision", document_id: "source", relative_path: "sources/source.md", sha256: "source-hash", size: 1, predecessor: null }, bounded_text: null };
  const target: VaultDocument = { id: "target", vault_id: "v", kind: "note", title: "target", current_scope: "private-data", state: "active", current_revision_id: "target-revision" };
  try {
    await act(async () => { root.render(<VaultDocumentDetail detail={sourceDetail} documents={[target]} host="local" onArchive={() => {}} onChanged={async () => {}} />); });
    await act(async () => { find(node, "노트 만들기").click(); });
    await act(async () => { setText(node.querySelector("textarea")!, "draft"); });

    expect((node.querySelector("select[aria-label='연결할 기존 노트']") as HTMLSelectElement).disabled).toBe(true);
    expect(mocks.detail).not.toHaveBeenCalled();
  } finally { await act(async () => { root.unmount(); }); }
});

function noteDetail(extra: Partial<Detail> = {}): Detail {
  const base: Detail = {
    document: { id: "note", vault_id: "v", kind: "note", title: "note", current_scope: "private-data", state: "active", current_revision_id: "revision" },
    current_revision: { id: "revision", document_id: "note", relative_path: "notes/n.md", sha256: "note-hash", size: 4, predecessor: null },
    revision_history: [], source_revisions: [], backlinks: [], bounded_text: "body", read_error: null, unsupported_reason: null, index_status: "indexed", index_reason: null,
  };
  return { ...base, ...extra };
}
function find(node: HTMLElement, text: string) { return Array.from(node.querySelectorAll<HTMLButtonElement>("button")).find(button => button.textContent === text)!; }
function setText(element: HTMLTextAreaElement, value: string) { Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(element, value); element.dispatchEvent(new Event("input", { bubbles: true })); }
