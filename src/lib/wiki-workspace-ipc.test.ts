import { beforeEach, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { wikiGraph, wikiRead, wikiSave, wikiTrash } from "./wiki-workspace-ipc";
beforeEach(() => invoke.mockClear());
it("does not send remote vault paths to local IPC", () => {
  for (const call of [() => wikiGraph("v", "remote"), () => wikiRead("v", "x.md", "remote"), () => wikiSave("v", "x.md", "x", null, "remote"), () => wikiTrash("v", "x.md", "hash", "remote")]) expect(call).toThrow("로컬");
  expect(invoke).not.toHaveBeenCalled();
});
it("carries the vault identity and expected content hash for file mutations", async () => {
  await wikiSave("v", "개인/문서.md", "# 문서", "base", "local");
  expect(invoke).toHaveBeenLastCalledWith("wiki_workspace_save", { vaultId: "v", path: "개인/문서.md", content: "# 문서", expectedHash: "base" });
  await wikiTrash("v", "개인/문서.md", "base", "local");
  expect(invoke).toHaveBeenLastCalledWith("wiki_workspace_trash", { vaultId: "v", path: "개인/문서.md", expectedHash: "base" });
});
