// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const mocks = vi.hoisted(() => ({ status: vi.fn(), documents: vi.fn(), scan: vi.fn(), graph: vi.fn(), read: vi.fn(), settings: vi.fn() }));
vi.mock("../../lib/knowledge-vault-ipc", () => ({ vaultStatus: mocks.status, vaultDocuments: mocks.documents, vaultScan: mocks.scan, vaultSettingsGet: mocks.settings }));
vi.mock("../../lib/wiki-workspace-ipc", () => ({ wikiGraph: mocks.graph, wikiRead: mocks.read }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
vi.mock("./VaultSettings", () => ({ VaultSettings: () => null }));
vi.mock("./VaultFolderSettings", () => ({ VaultFolderSettings: () => null }));
vi.mock("./VaultAddSource", () => ({ VaultAddSource: () => null }));
vi.mock("./VaultMemoryPanel", () => ({ VaultMemoryPanel: () => null }));
import { VaultView } from "./VaultView";
let node: HTMLDivElement; let root: Root;
const page = { id: "a.md", path: "a.md", title: "파일 문서", body: "# 파일 문서", source_prefix: "", aliases: [], tags: [], type: "page", status: "", scope: "", outgoing: [], backlinks: [], sha256: "h" };
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
beforeEach(() => { Object.values(mocks).forEach(m => m.mockReset()); mocks.status.mockResolvedValue({ supported: true, vaults: [{ id: "v", vault_root: "/vault", enabled: true }] }); mocks.documents.mockResolvedValue([]); mocks.settings.mockResolvedValue({ wiki_dir: "wiki", organizer_skill: "wiki-organizer", wiki_home: "위키-시작.md" }); mocks.scan.mockResolvedValue({ indexed: 0, skipped: 0, warnings: [] }); mocks.graph.mockResolvedValue({ schema_version: 1, nodes: [page], edges: [], diagnostics: [], writable: true }); mocks.read.mockResolvedValue({ path: "a.md", content: "# 파일 문서", sha256: "h" }); node = document.createElement("div"); document.body.append(node); root = createRoot(node); });
afterEach(() => { act(() => root.unmount()); node.remove(); });
async function render() { await act(async () => { root.render(<VaultView host="local" />); await flush(); }); await act(flush); }

it("waits for the initial graph read before scanning without a spurious dirty warning", async () => {
  await render();
  expect(mocks.scan).toHaveBeenCalledOnce();
  expect(node.querySelector("[role='alert']")).toBeNull();
  expect(node.querySelector("[aria-label='문서와 그래프']")).not.toBeNull();
});
it("keeps the real file editor mounted when the parent tab navigation is attempted", async () => {
  await render();
  await act(async () => { (node.querySelector("[aria-label='위키 문서 목록'] nav button") as HTMLButtonElement).click(); });
  await act(async () => { Array.from(node.querySelectorAll<HTMLButtonElement>("button")).find(b => b.textContent === "원문 편집")!.click(); await flush(); });
  const editor = node.querySelector("textarea"); expect(editor).not.toBeNull();
  await act(async () => { Array.from(node.querySelectorAll<HTMLButtonElement>("button")).find(b => b.textContent === "메모리")!.click(); await flush(); });
  expect(node.querySelector("textarea")).toBe(editor);
  expect(node.textContent).toContain("수정 또는 저장 중인 내용이 있습니다");
});
