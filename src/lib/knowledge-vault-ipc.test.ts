import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { vaultSettingsGet, vaultSettingsSet, vaultScan, vaultSessionOpen } from "./knowledge-vault-ipc";

beforeEach(() => invoke.mockReset());

it("reads the vault folder settings without arguments", async () => {
  invoke.mockResolvedValue({ wiki_dir: "문서/기술-위키/wiki", organizer_skill: "knowledge-harness", wiki_home: "위키-시작.md" });

  await expect(vaultSettingsGet()).resolves.toEqual({ wiki_dir: "문서/기술-위키/wiki", organizer_skill: "knowledge-harness", wiki_home: "위키-시작.md" });

  expect(invoke).toHaveBeenCalledWith("knowledge_vault_settings_get");
});

it("sends the wiki directory, organizer skill and entry document under the camelCase names the command takes", async () => {
  invoke.mockResolvedValue({ wiki_dir: "wiki", organizer_skill: "wiki-organizer", wiki_home: "위키-시작.md" });

  await vaultSettingsSet("wiki", "wiki-organizer", "위키-시작.md");

  expect(invoke).toHaveBeenCalledWith("knowledge_vault_settings_set", { wikiDir: "wiki", organizerSkill: "wiki-organizer", wikiHome: "위키-시작.md" });
});

it("sends a scan with its exclusions and scope", async () => {
  invoke.mockResolvedValue({ indexed: 1, skipped: 0, partial: false, warnings: [] });

  await vaultScan("vault", [], "private-data", "/repo");

  expect(invoke).toHaveBeenCalledWith("knowledge_vault_scan", { vaultId: "vault", exclusions: [], scope: "private-data", repoRoot: "/repo" });
});

it("rejects remote settings writes before making a local IPC call", () => {
  expect(() => vaultSettingsSet("wiki", "wiki-organizer", "위키-시작.md", "remote")).toThrow("로컬 세션에서만");
  expect(() => vaultSessionOpen("vault", "claude", "remote")).toThrow("로컬 세션에서만");

  expect(invoke).not.toHaveBeenCalled();
});
