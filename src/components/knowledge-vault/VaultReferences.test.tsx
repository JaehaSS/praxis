// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const mocks = vi.hoisted(() => ({ preview: vi.fn(), exclude: vi.fn(), policy: vi.fn(), usage: vi.fn(), status: vi.fn(), documents: vi.fn() }));
vi.mock("../../lib/knowledge-vault-ipc", () => ({ vaultPreview: mocks.preview, vaultExcludePreviewReference: mocks.exclude, vaultSetDraftPolicy: mocks.policy, vaultUsage: mocks.usage, vaultStatus: mocks.status, vaultDocuments: mocks.documents }));

import { VaultReferences } from "./VaultReferences";

it("starts collapsed and toggles references and delivery history without resetting input policy", async () => {
  mocks.usage.mockResolvedValue([]);
  mocks.policy.mockResolvedValue(undefined);
  const pending = vi.fn();
  const node = document.createElement("div"); const root = createRoot(node);
  try {
    await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="find" clientRef="ref" taskId={9} onVaultMutationPendingChange={pending} />); });
    const details = node.querySelector("details");
    expect(details, "작업 참고 자료를 접고 펼칠 컨트롤이 있어야 한다").not.toBeNull();
    const summary = details!.querySelector("summary")!;
    expect(summary.textContent).toContain("작업 참고 자료");
    expect(details!.open).toBe(false);
    await act(async () => { summary.click(); });
    expect(details!.open).toBe(true);
    expect(details!.textContent).toContain("자료 전달 기록");
    const taskOnly = Array.from(details!.querySelectorAll("button")).find(button => button.textContent === "이번 작업만")!;
    await act(async () => { taskOnly.click(); });
    expect(taskOnly.disabled).toBe(true);
    await act(async () => { summary.click(); });
    expect(details!.open).toBe(false);
    await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="changed" clientRef="ref" taskId={9} onVaultMutationPendingChange={pending} />); });
    expect(pending).toHaveBeenLastCalledWith(true);
    expect(summary.textContent).toContain("확인 필요");
    await act(async () => { summary.click(); });
    expect(details!.open).toBe(true);
    expect(taskOnly.disabled).toBe(false);
  } finally {
    await act(async () => { root.unmount(); });
    vi.clearAllMocks();
  }
});

it("never invokes the local vault from a Runner host", async () => {
  const node = document.createElement("div"); const root = createRoot(node);
  await act(async () => { root.render(<VaultReferences host="runner" repo="/repo" query="find" clientRef="ref" taskId={9} />); });
  expect(mocks.preview).not.toHaveBeenCalled();
  expect(mocks.usage).not.toHaveBeenCalled();
  await act(async () => { root.unmount(); });
});

it("clears stale previews, excludes references, and labels actual usage", async () => {
  mocks.usage.mockResolvedValue([{ attempt_id: "attempt", revision_id: "used", revision_hash: "hash", snippet: "actual", snippet_hash: "snippet-hash", delivery_state: "delivered", citation_state: "unknown" }]);
  mocks.preview.mockResolvedValue({ id: "preview", query_hash: "query", references: [{ document_id: "doc", title: "Reference", scope: "project", revision_id: "revision", revision_hash: "hash", snippet: "candidate", reason: "matches request", excluded: false, stale_reason: null }] });
  mocks.exclude.mockResolvedValue(undefined);
  const node = document.createElement("div"); const root = createRoot(node);
  await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="find" clientRef="create-ref" taskId={9} />); });
  await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "관련 자료 미리보기") as HTMLButtonElement).click(); });
  expect(mocks.preview).toHaveBeenCalledWith("/repo", "find", "create-ref", "local");
  expect(mocks.usage).toHaveBeenCalledWith(9, "local");
  expect(node.textContent).toContain("작업에 전달됨");
  expect(node.textContent).toContain("인용 여부 알 수 없음");
  await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "제외") as HTMLButtonElement).click(); });
  expect(mocks.exclude).toHaveBeenCalledWith("preview", "revision", "local");
  expect(mocks.preview).toHaveBeenCalledTimes(1);
  expect(node.textContent).toContain("제외됨");
  expect(node.textContent).toContain("Reference");
  await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="changed" clientRef="create-ref" taskId={9} />); });
  expect(node.textContent).not.toContain("Reference");
  await act(async () => { root.unmount(); });
});

it("persists task-only input policy before a local send can proceed", async () => {
  mocks.policy.mockResolvedValue(undefined);
  const node = document.createElement("div"); const root = createRoot(node);
  await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="find" clientRef="create-ref" />); });
  await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "이번 작업만") as HTMLButtonElement).click(); });
  expect(mocks.policy).toHaveBeenCalledWith("/repo", "find", "create-ref", "task_only", [], "local");
  await act(async () => { root.unmount(); });
});

it("blocks sending until a stale restrictive policy is reviewed again", async () => {
  mocks.policy.mockResolvedValue(undefined);
  const pending = vi.fn();
  const node = document.createElement("div"); const root = createRoot(node);
  await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="find" clientRef="create-ref" onVaultMutationPendingChange={pending} />); });
  await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "이번 작업만") as HTMLButtonElement).click(); });
  await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="changed" clientRef="create-ref" onVaultMutationPendingChange={pending} />); });
  expect(pending).toHaveBeenLastCalledWith(true);
  expect((Array.from(node.querySelectorAll("button")).find(button => button.textContent === "이번 작업만") as HTMLButtonElement).disabled).toBe(false);
  await act(async () => { root.unmount(); });
});

it("clears restrictive state when its task context changes", async () => {
  mocks.policy.mockResolvedValue(undefined);
  const pending = vi.fn();
  const node = document.createElement("div"); const root = createRoot(node);
  await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="find" clientRef="create-ref" onVaultMutationPendingChange={pending} />); });
  await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "이번 작업만") as HTMLButtonElement).click(); });
  await act(async () => { root.render(<VaultReferences host="runner" repo="/repo" query="find" clientRef="create-ref" onVaultMutationPendingChange={pending} />); });
  expect(pending).toHaveBeenLastCalledWith(false);
  await act(async () => { root.unmount(); });
});

it("binds a selected private revision before it can be sent", async () => {
  mocks.status.mockResolvedValue({ vaults: [{ id: "vault", enabled: true }] });
  mocks.documents.mockResolvedValue([{ id: "doc", vault_id: "vault", kind: "source", title: "Private", state: "active", current_revision_id: "revision", current_revision_hash: "hash", current_scope: "private-data" }]);
  mocks.policy.mockResolvedValue(undefined);
  const node = document.createElement("div"); const root = createRoot(node);
  await act(async () => { root.render(<VaultReferences host="local" repo="/repo" query="find" clientRef="create-ref" />); });
  await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "나만 보기 자료 첨부") as HTMLButtonElement).click(); });
  await act(async () => { (Array.from(node.querySelectorAll("button")).find(button => button.textContent === "첨부") as HTMLButtonElement).click(); });
  expect(mocks.policy).toHaveBeenCalledWith("/repo", "find", "create-ref", "private_attachment", ["revision"], "local");
  expect(node.textContent).toContain("hash");
  await act(async () => { root.unmount(); });
});
