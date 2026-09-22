import { describe, expect, it, vi } from "vitest";
import { archiveCurrentMemoryResults } from "./memory-bulk-archive";

const currentResults = [
  { id: 1, status: "candidate" },
  { id: 2, status: "pending_review" },
  { id: 3, status: "verified" },
  { id: 4, status: "archived" },
] as const;

describe("archiveCurrentMemoryResults", () => {
  it("archives only eligible rows in the supplied current result", async () => {
    const askConfirmation = vi.fn((_message: string) => true);
    const archive = vi.fn(async (_id: number) => undefined);

    const archived = await archiveCurrentMemoryResults(currentResults, {
      askConfirmation,
      archive,
    });

    expect(archived).toBe(2);
    expect(archive.mock.calls).toEqual([[1], [3]]);
    expect(askConfirmation.mock.calls[0]?.[0]).toContain("현재 결과 2건");
    expect(askConfirmation.mock.calls[0]?.[0]).toContain("본문·근거·이력은 유지");
  });

  it("performs no mutation when the user cancels", async () => {
    const archive = vi.fn(async (_id: number) => undefined);

    const archived = await archiveCurrentMemoryResults(currentResults, {
      askConfirmation: () => false,
      archive,
    });

    expect(archived).toBe(0);
    expect(archive).not.toHaveBeenCalled();
  });

  it("does not ask when no archivable result remains", async () => {
    const askConfirmation = vi.fn((_message: string) => true);
    const archive = vi.fn(async (_id: number) => undefined);

    const archived = await archiveCurrentMemoryResults(
      [{ id: 2, status: "pending_review" }, { id: 4, status: "archived" }],
      { askConfirmation, archive },
    );

    expect(archived).toBe(0);
    expect(askConfirmation).not.toHaveBeenCalled();
    expect(archive).not.toHaveBeenCalled();
  });

  it("reports the failed id and completed progress", async () => {
    const archive = vi
      .fn<(id: number) => Promise<void>>()
      .mockResolvedValueOnce()
      .mockRejectedValueOnce(new Error("runner unavailable"));

    await expect(
      archiveCurrentMemoryResults(currentResults, {
        askConfirmation: () => true,
        archive,
      }),
    ).rejects.toThrow("memory #3 보관 실패 · 1/2건 완료");
  });
});
