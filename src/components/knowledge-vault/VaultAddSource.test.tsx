// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const mocks = vi.hoisted(() => ({ text: vi.fn(), files: vi.fn(), open: vi.fn() }));
vi.mock("../../lib/knowledge-vault-ipc", () => ({ vaultCreateTextSource: mocks.text, vaultCreateUrlSource: vi.fn(), vaultImportFiles: mocks.files }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));
import { VaultAddSource } from "./VaultAddSource";

let node: HTMLDivElement; let root: Root;
beforeEach(() => { mocks.text.mockReset(); mocks.files.mockReset(); mocks.open.mockReset(); mocks.text.mockResolvedValue({ document_id: "d", revision_id: "r" }); node = document.createElement("div"); document.body.appendChild(node); root = createRoot(node); });
afterEach(() => { act(() => root.unmount()); node.remove(); });

it("selects successfully added text for authoring", async () => {
  const saved = vi.fn().mockResolvedValue(undefined);
  await act(async () => { root.render(<VaultAddSource vaultId="vault" onNote={async () => {}} onSaved={saved} />); });
  await act(async () => { find("자료 추가").click(); });
  const [title, body] = node.querySelectorAll("input, textarea") as unknown as [HTMLInputElement, HTMLTextAreaElement];
  await act(async () => { input(title, "research"); input(body, "body"); find("추가").click(); });
  expect(mocks.text).toHaveBeenCalledWith("vault", "research", "body", "private-data", undefined, "local");
  expect(saved).toHaveBeenCalledWith(["r"]);
});

it("retries only failed file paths after a partial import", async () => {
  mocks.open.mockResolvedValue(["/vault/kept.md", "/vault/retry.md"]);
  mocks.files
    .mockResolvedValueOnce([
      { path: "/vault/kept.md", source: { document_id: "kept", revision_id: "r-kept", sha256: "hash" }, error: null },
      { path: "/vault/retry.md", source: null, error: "읽지 못했습니다" },
    ])
    .mockResolvedValueOnce([
      { path: "/vault/retry.md", source: { document_id: "retry", revision_id: "r-retry", sha256: "hash" }, error: null },
    ]);
  const saved = vi.fn().mockResolvedValue(undefined);

  await act(async () => { root.render(<VaultAddSource vaultId="vault" onNote={async () => {}} onSaved={saved} />); });
  await act(async () => { find("자료 추가").click(); });
  await act(async () => { button("파일").click(); });
  await act(async () => { button("파일 고르기").click(); await Promise.resolve(); });
  await act(async () => { button("추가").click(); await Promise.resolve(); });

  expect(mocks.files).toHaveBeenLastCalledWith("vault", ["/vault/kept.md", "/vault/retry.md"], "private-data", undefined, "local");
  expect(saved).toHaveBeenLastCalledWith(["r-kept"]);
  expect(node.textContent).toContain("일부 자료를 추가하지 못했습니다.");

  await act(async () => { button("추가").click(); await Promise.resolve(); });

  expect(mocks.files).toHaveBeenLastCalledWith("vault", ["/vault/retry.md"], "private-data", undefined, "local");
  expect(saved).toHaveBeenLastCalledWith(["r-retry"]);
});

it("does not import files when file selection is cancelled", async () => {
  mocks.open.mockResolvedValue(null);
  await act(async () => { root.render(<VaultAddSource vaultId="vault" onNote={async () => {}} />); });
  await act(async () => { find("자료 추가").click(); });
  await act(async () => { button("파일").click(); });
  await act(async () => { button("파일 고르기").click(); await Promise.resolve(); });

  expect(mocks.files).not.toHaveBeenCalled();
  expect(button("추가").disabled).toBe(true);
});

function find(text: string) { return Array.from(node.querySelectorAll<HTMLButtonElement>("button")).find(button => button.textContent === text)!; }
function button(text: string) { return Array.from(node.querySelectorAll<HTMLButtonElement>("button")).find(item => item.textContent === text)!; }
function input(element: HTMLInputElement | HTMLTextAreaElement, value: string) { Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), "value")!.set!.call(element, value); element.dispatchEvent(new Event("input", { bubbles: true })); }
