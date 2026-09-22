import type { Memory } from "../lib/ipc";

type MigrationMemory = Pick<Memory, "id" | "status">;

interface LegacyArchiveDeps {
  askConfirmation: (message: string) => boolean;
  archive: (id: number) => Promise<void>;
}

const ARCHIVABLE_STATUSES = new Set<Memory["status"]>([
  "candidate",
  "verified",
  "stale",
  "rejected",
  "legacy_unverified",
]);

export function canArchiveMemory(status: Memory["status"]): boolean {
  return ARCHIVABLE_STATUSES.has(status);
}

export async function archiveLegacySelection(
  memories: readonly MigrationMemory[],
  selected: ReadonlySet<number>,
  deps: LegacyArchiveDeps,
): Promise<number> {
  const ids = memories
    .filter((memory) => memory.status === "legacy_unverified" && selected.has(memory.id))
    .map((memory) => memory.id);
  if (ids.length === 0) return 0;

  const confirmed = deps.askConfirmation(
    `${ids.length}건을 이관 대상에서 제외하고 보관할까요? 본문·근거·이력은 유지됩니다.`,
  );
  if (!confirmed) return 0;

  for (const [index, id] of ids.entries()) {
    try {
      await deps.archive(id);
    } catch (error) {
      throw new Error(
        `legacy memory #${id} 보관 실패 · ${index}/${ids.length}건 완료: ${String(error)}`,
      );
    }
  }
  return ids.length;
}
