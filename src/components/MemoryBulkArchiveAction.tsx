import type { ReactElement } from "react";

interface MemoryBulkArchiveActionProps {
  count: number;
  busy: boolean;
  onArchive: () => void;
}

export function MemoryBulkArchiveAction({
  count,
  busy,
  onArchive,
}: MemoryBulkArchiveActionProps): ReactElement | null {
  if (count === 0) return null;

  return (
    <button
      type="button"
      title="현재 검색·프로젝트·상태 필터 결과 중 보관 가능한 항목만 처리합니다."
      className="h-8 rounded-md border border-border bg-status-awaiting/15 px-3 text-sm text-status-awaiting disabled:opacity-50"
      disabled={busy}
      onClick={onArchive}
    >
      {busy ? "현재 결과 보관 중…" : `현재 결과 ${count}건 보관`}
    </button>
  );
}
