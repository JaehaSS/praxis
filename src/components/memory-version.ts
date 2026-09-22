import type {
  Memory,
  MemoryStatus,
  MemoryVersion,
} from "../lib/ipc";

export type MemoryVersionLoadState =
  | { memoryId: number; status: "loading" }
  | { memoryId: number; status: "ready"; versions: MemoryVersion[] }
  | { memoryId: number; status: "unsupported" }
  | { memoryId: number; status: "error"; message: string };

export interface MemoryVersionRestoreDeps {
  askConfirmation: (message: string) => boolean;
  restore: (
    id: number,
    sourceVersion: number,
    expectedCurrentVersion: number,
    expectedStatus: MemoryStatus,
  ) => Promise<number>;
}

export function versionStateForMemory(
  state: MemoryVersionLoadState,
  memoryId: number,
): MemoryVersionLoadState {
  if (state.memoryId === memoryId) return state;
  return { memoryId, status: "loading" };
}

export function isVersionHistoryUnsupported(reason: unknown): boolean {
  if (typeof reason !== "object" || reason === null || !("status" in reason)) {
    return false;
  }
  return reason.status === 404;
}

export function canRestoreMemoryVersion(
  memory: Memory,
  version: MemoryVersion,
): boolean {
  if (version.memory_id !== memory.id) return false;
  if (version.version < memory.current_version) return true;
  return (
    version.version === memory.current_version
    && memory.status === "archived"
  );
}

export function defaultComparedVersion(
  memory: Memory,
  versions: MemoryVersion[],
): MemoryVersion | null {
  return versions.find((version) => canRestoreMemoryVersion(memory, version))
    ?? versions.find((version) => version.version === memory.current_version)
    ?? null;
}

export async function restoreMemoryVersion(
  memory: Memory,
  version: MemoryVersion,
  deps: MemoryVersionRestoreDeps,
): Promise<boolean> {
  if (!canRestoreMemoryVersion(memory, version)) {
    throw new Error("복원 가능한 메모리 버전이 아닙니다");
  }
  const nextVersion = memory.current_version + 1;
  const confirmed = deps.askConfirmation(
    `v${version.version} 내용을 새 v${nextVersion} 후보로 복원할까요?\n`
    + "현재 이력은 보존되며, 과거 버전의 근거와 승인은 재사용하지 않습니다. 복원 후 다시 검토해야 합니다.",
  );
  if (!confirmed) return false;
  await deps.restore(
    memory.id,
    version.version,
    memory.current_version,
    memory.status,
  );
  return true;
}
