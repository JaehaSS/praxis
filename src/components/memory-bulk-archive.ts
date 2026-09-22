import type { Memory } from "../lib/ipc";
import { canArchiveMemory } from "./memory-migration";

type CurrentResultMemory = Pick<Memory, "id" | "status">;

interface BulkArchiveDeps {
  askConfirmation: (message: string) => boolean;
  archive: (id: number) => Promise<void>;
}

export function countArchivableMemories(
  memories: readonly CurrentResultMemory[],
): number {
  return memories.filter((memory) => canArchiveMemory(memory.status)).length;
}

export async function archiveCurrentMemoryResults(
  memories: readonly CurrentResultMemory[],
  deps: BulkArchiveDeps,
): Promise<number> {
  const ids = memories
    .filter((memory) => canArchiveMemory(memory.status))
    .map((memory) => memory.id);
  if (ids.length === 0) return 0;

  const confirmed = deps.askConfirmation(
    `현재 결과 ${ids.length}건을 보관할까요? 본문·근거·이력은 유지되며 보관됨 필터에서 다시 확인할 수 있습니다.`,
  );
  if (!confirmed) return 0;

  for (const [index, id] of ids.entries()) {
    try {
      await deps.archive(id);
    } catch (error) {
      throw new Error(
        `memory #${id} 보관 실패 · ${index}/${ids.length}건 완료: ${String(error)}`,
      );
    }
  }
  return ids.length;
}
