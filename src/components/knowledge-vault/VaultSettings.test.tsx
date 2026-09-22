// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
const mocks = vi.hoisted(() => ({ open: vi.fn(), connect: vi.fn(), status: vi.fn(), documents: vi.fn(), recover: vi.fn(), rebind: vi.fn(), rebindProject: vi.fn(), registerProject: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));
vi.mock("../../lib/knowledge-vault-ipc", () => ({ vaultStatus: mocks.status, vaultConnect: mocks.connect, vaultDisconnect: vi.fn(), vaultDocuments: mocks.documents, vaultRebind: mocks.rebind, vaultRebindProject: mocks.rebindProject, vaultRecoverOperations: mocks.recover, vaultRegisterProject: mocks.registerProject }));
import { VaultSettings } from "./VaultSettings";

const connected = { supported: true, vaults: [{ id: "v", vault_root: "/vault", enabled: true }], prior_bindings: [], operation_conflicts: [], current_provider: "provider", current_provider_display: "Provider", provider_available: true, current_consent: null, index_rebuild_needed: false };

it("does not connect when directory selection is cancelled", async () => { mocks.status.mockResolvedValue({ ...connected, vaults: [], project_binding: null }); mocks.open.mockResolvedValue(null); const node = document.createElement("div"); const root = createRoot(node); await act(async () => { root.render(<VaultSettings host="local" />); }); await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "디렉터리 선택") as HTMLButtonElement).click(); }); expect(mocks.connect).not.toHaveBeenCalled(); root.unmount(); });

it("registers the current project without asking for capture consent", async () => {
  // 제안 파이프라인이 사라졌다 — 남는 것은 등록뿐이고, 동의 스위치는 다시 나타나면 안 된다.
  mocks.registerProject.mockResolvedValue({ id: "p", epoch: "1", canonical_repo_root: "/repo" });
  mocks.status.mockResolvedValue({ ...connected, project_binding: null });
  const node = document.createElement("div");
  const root = createRoot(node);
  await act(async () => { root.render(<VaultSettings host="local" repo="/repo" />); });

  expect(node.textContent).not.toContain("분석 동의");
  expect(node.textContent).not.toContain("작업 완료 후 지식 제안");
  expect(node.querySelector('input[type="checkbox"]')).toBeNull();

  await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "현재 프로젝트 등록") as HTMLButtonElement).click(); });

  expect(mocks.registerProject).toHaveBeenCalledWith("/repo", "local");
  await act(async () => root.unmount());
});

it("offers project rebinding once the project is bound", async () => {
  mocks.status.mockResolvedValue({ ...connected, project_binding: { id: "p", epoch: "1", canonical_repo_root: "/repo" } });
  const node = document.createElement("div");
  const root = createRoot(node);
  await act(async () => { root.render(<VaultSettings host="local" repo="/repo" />); });

  expect(Array.from(node.querySelectorAll("button")).some(button => button.textContent === "새 프로젝트 위치 선택")).toBe(true);
  expect(Array.from(node.querySelectorAll("button")).some(button => button.textContent === "현재 프로젝트 등록")).toBe(false);
  await act(async () => root.unmount());
});

it("rebinds with every listed revision after '검색 결과 모두 확인', and allows an unconfirmed rest", async () => {
  // 백엔드는 미확인 revision을 막지 않고 grant만 회수한다 — UI도 하나만 골라도, 전부 골라도 보낼 수 있어야 한다.
  mocks.status.mockResolvedValue({ ...connected, vaults: [{ id: "v", vault_root: "/old", enabled: false }], project_binding: null });
  mocks.documents.mockResolvedValue([
    { id: "d1", vault_id: "v", kind: "source", title: "proxy-common", state: "active", current_revision_id: "r1", current_revision_hash: "h1", current_scope: "private-data" },
    { id: "d2", vault_id: "v", kind: "source", title: "README", state: "active", current_revision_id: "r2", current_revision_hash: "h2", current_scope: "private-data" },
    { id: "d3", vault_id: "v", kind: "note", title: "no head", state: "missing", current_revision_id: null, current_revision_hash: null, current_scope: "private-data" },
  ]);
  mocks.open.mockResolvedValue("/new");
  mocks.rebind.mockResolvedValue({ id: "v", vault_root: "/new", enabled: true });
  const node = document.createElement("div");
  const root = createRoot(node);
  const button = (text: string) => Array.from(node.querySelectorAll("button")).find(item => item.textContent === text) as HTMLButtonElement;
  await act(async () => { root.render(<VaultSettings host="local" />); });
  await act(async () => { button("기존 자료함 다시 연결: /old").click(); });
  await act(async () => { button("새 디렉터리 선택").click(); });

  expect(node.textContent).toContain("확인 0 / 2");
  await act(async () => { button("검색 결과 모두 확인").click(); });
  expect(node.textContent).toContain("확인 2 / 2");
  expect(Array.from(node.querySelectorAll('input[type="checkbox"]')).every(input => (input as HTMLInputElement).checked)).toBe(true);
  await act(async () => { button("모두 해제").click(); });
  expect(node.textContent).toContain("확인 0 / 2");
  expect(button("새 위치로 다시 연결").disabled).toBe(false);

  await act(async () => { (node.querySelectorAll('input[type="checkbox"]')[0] as HTMLInputElement).click(); });
  await act(async () => { button("새 위치로 다시 연결").click(); });
  expect(mocks.rebind).toHaveBeenCalledWith("v", "/new", ["r1"], "local");
  await act(async () => root.unmount());
});
