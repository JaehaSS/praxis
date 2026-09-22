// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const mocks = vi.hoisted(() => ({ get: vi.fn(), set: vi.fn() }));
vi.mock("../../lib/knowledge-vault-ipc", () => ({ vaultSettingsGet: mocks.get, vaultSettingsSet: mocks.set }));
import { VaultFolderSettings } from "./VaultFolderSettings";

let node: HTMLDivElement; let root: Root;
beforeEach(() => { mocks.get.mockReset(); mocks.set.mockReset(); node = document.createElement("div"); document.body.appendChild(node); root = createRoot(node); mocks.get.mockResolvedValue({ wiki_dir: "wiki", organizer_skill: "wiki-organizer", wiki_home: "위키-시작.md" }); });
afterEach(() => { act(() => root.unmount()); node.remove(); });

it("shows the stored folder, entry document and skill", async () => {
  mocks.get.mockResolvedValue({ wiki_dir: "문서/기술-위키/wiki", organizer_skill: "knowledge-harness", wiki_home: "위키-시작.md" });

  await render();

  expect(field("위키 폴더").value).toBe("문서/기술-위키/wiki");
  expect(field("진입 문서").value).toBe("위키-시작.md");
  expect(field("정리 스킬").value).toBe("knowledge-harness");
});

it("saves edited values and asks for a rescan", async () => {
  mocks.set.mockResolvedValue({ wiki_dir: "문서/기술-위키/wiki", organizer_skill: "knowledge-harness", wiki_home: "시작.md" });

  await render();
  await act(async () => { type(field("위키 폴더"), " 문서/기술-위키/wiki "); type(field("정리 스킬"), "knowledge-harness"); type(field("진입 문서"), " 시작.md "); });
  await act(async () => { button("저장").click(); await flush(); });

  expect(mocks.set).toHaveBeenCalledWith("문서/기술-위키/wiki", "knowledge-harness", "시작.md", "local");
  expect(field("진입 문서").value).toBe("시작.md");
  expect(node.querySelector("[role=status]")?.textContent).toContain("다시 스캔");
});

it("keeps the form usable when saving fails", async () => {
  mocks.set.mockRejectedValue("설정을 저장하지 못했습니다.");

  await render();
  await act(async () => { button("저장").click(); await flush(); });

  expect(node.querySelector("[role=alert]")?.textContent).toContain("설정을 저장하지 못했습니다.");
  expect(button("저장").disabled).toBe(false);
});

it("does not read local settings on a remote host", async () => {
  await act(async () => { root.render(<VaultFolderSettings host="remote" />); });

  expect(mocks.get).not.toHaveBeenCalled();
  expect(node.textContent).toBe("");
});

async function render() { await act(async () => { root.render(<VaultFolderSettings host="local" />); await flush(); }); }
async function flush() { await Promise.resolve(); await Promise.resolve(); }
function field(label: string) { return node.querySelector<HTMLInputElement>(`input[aria-label='${label}']`)!; }
function button(text: string) { return Array.from(node.querySelectorAll<HTMLButtonElement>("button")).find(item => item.textContent === text)!; }
function type(element: HTMLInputElement, value: string) { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(element, value); element.dispatchEvent(new Event("input", { bubbles: true })); }
