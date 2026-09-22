import { describe, expect, it, vi } from "vitest";
import {
  archiveLegacySelection,
  canArchiveMemory,
} from "./memory-migration";

const memories = [
  { id: 1, status: "legacy_unverified" },
  { id: 2, status: "candidate" },
  { id: 3, status: "legacy_unverified" },
] as const;

describe("archiveLegacySelection", () => {
  it("archives only selected legacy rows and explains audit retention", async () => {
    const askConfirmation = vi.fn((_message: string) => true);
    const archive = vi.fn(async (_id: number) => undefined);

    const archived = await archiveLegacySelection(
      memories,
      new Set([1, 2, 3]),
      { askConfirmation, archive },
    );

    expect(archived).toBe(2);
    expect(archive.mock.calls).toEqual([[1], [3]]);
    expect(askConfirmation.mock.calls[0]?.[0]).toContain("본문·근거·이력은 유지");
  });

  it("performs no mutation when the user cancels", async () => {
    const archive = vi.fn(async (_id: number) => undefined);

    const archived = await archiveLegacySelection(
      memories,
      new Set([1, 3]),
      { askConfirmation: () => false, archive },
    );

    expect(archived).toBe(0);
    expect(archive).not.toHaveBeenCalled();
  });

  it("does not ask for confirmation when no selected legacy row remains", async () => {
    const askConfirmation = vi.fn((_message: string) => true);
    const archive = vi.fn(async (_id: number) => undefined);

    const archived = await archiveLegacySelection(
      memories,
      new Set([2, 999]),
      { askConfirmation, archive },
    );

    expect(archived).toBe(0);
    expect(askConfirmation).not.toHaveBeenCalled();
    expect(archive).not.toHaveBeenCalled();
  });

  it("reports completed progress when a later archive call fails", async () => {
    const archive = vi
      .fn<(id: number) => Promise<void>>()
      .mockResolvedValueOnce()
      .mockRejectedValueOnce(new Error("runner unavailable"));

    await expect(
      archiveLegacySelection(memories, new Set([1, 3]), {
        askConfirmation: () => true,
        archive,
      }),
    ).rejects.toThrow("1/2건 완료");
  });
});

describe("canArchiveMemory", () => {
  it("matches the backend lifecycle states that allow archive", () => {
    expect(canArchiveMemory("legacy_unverified")).toBe(true);
    expect(canArchiveMemory("candidate")).toBe(true);
    expect(canArchiveMemory("verified")).toBe(true);
    expect(canArchiveMemory("stale")).toBe(true);
    expect(canArchiveMemory("rejected")).toBe(true);
    expect(canArchiveMemory("pending_review")).toBe(false);
    expect(canArchiveMemory("archived")).toBe(false);
  });
});
