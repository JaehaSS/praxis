import { describe, expect, it, vi } from "vitest";
import type { Memory, MemoryVersion } from "../lib/ipc";
import {
  canRestoreMemoryVersion,
  isVersionHistoryUnsupported,
  restoreMemoryVersion,
  versionStateForMemory,
  type MemoryVersionLoadState,
} from "./memory-version";

const memory = (status: Memory["status"] = "candidate"): Memory => ({
  id: 7,
  tier: "project",
  scope_key: "/repo",
  kind: "claim",
  content: "current",
  source_session: null,
  confidence: 0,
  usage_count: 0,
  last_used: null,
  created_at: 1,
  knowledge_type: "claim",
  status,
  current_version: 2,
  utility_score: 0,
  review_due_at: null,
  verified_at: null,
  stale_at: null,
  archived_at: status === "archived" ? 2 : null,
  dormant: false,
});

const version = (value: number): MemoryVersion => ({
  memory_id: 7,
  version: value,
  content: value === 1 ? "previous" : "current",
  knowledge_type: "claim",
  scope_snapshot: "/repo",
  created_at: value,
  editor_kind: value === 1 ? "candidate_intake" : "human_edit",
  evidence_count: value,
});

describe("memory version workflow", () => {
  it("hides a previous memory history while the current memory loads", () => {
    const previous: MemoryVersionLoadState = {
      memoryId: 6,
      status: "ready",
      versions: [version(1)],
    };

    expect(versionStateForMemory(previous, 7)).toEqual({
      memoryId: 7,
      status: "loading",
    });
  });

  it("recognizes an older Runner that lacks version history", () => {
    expect(isVersionHistoryUnsupported({ status: 404 })).toBe(true);
    expect(isVersionHistoryUnsupported(new Error("offline"))).toBe(false);
  });

  it("restores historical versions and only the archived current version", () => {
    const statuses: Memory["status"][] = [
      "candidate",
      "pending_review",
      "verified",
      "stale",
      "rejected",
      "archived",
      "legacy_unverified",
    ];
    statuses.forEach((status) => {
      expect(canRestoreMemoryVersion(memory(status), version(1))).toBe(true);
    });
    expect(canRestoreMemoryVersion(memory(), version(2))).toBe(false);
    expect(canRestoreMemoryVersion(memory("archived"), version(2))).toBe(true);
  });

  it("keeps cancellation mutation-free and sends both CAS owners on confirmation", async () => {
    const restore = vi.fn(async () => 3);
    const askConfirmation = vi.fn((_message: string) => false);

    await expect(
      restoreMemoryVersion(memory(), version(1), { askConfirmation, restore }),
    ).resolves.toBe(false);
    expect(restore).not.toHaveBeenCalled();

    askConfirmation.mockReturnValue(true);
    await expect(
      restoreMemoryVersion(memory(), version(1), { askConfirmation, restore }),
    ).resolves.toBe(true);
    expect(restore).toHaveBeenCalledWith(7, 1, 2, "candidate");
    expect(askConfirmation.mock.calls[1][0]).toContain("새 v3 후보");
    expect(askConfirmation.mock.calls[1][0]).toContain("근거와 승인");
  });
});
