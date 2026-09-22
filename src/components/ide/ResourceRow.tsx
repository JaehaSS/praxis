import type { ReactNode } from "react";
import { deleteBtnCls } from "./formStyles";

/** on/off 토글 배지 — SchedulesView/McpServersView 공용 (순수 추출, 스타일 동일). */
export function ToggleBadge({ enabled, onToggle }: { enabled: boolean; onToggle: () => void }) {
  return (
    <button
      className={`text-xs font-medium ${enabled ? "text-status-done" : "text-text-muted"}`}
      onClick={onToggle}
      title="토글"
    >
      {enabled ? "● on" : "○ off"}
    </button>
  );
}

/** 등록된 리소스 한 건을 보여주는 행 카드 래퍼 — 세 뷰의 반복 카드 컨테이너 공용. */
export function ResourceRow({ children }: { children: ReactNode }) {
  return (
    <div className="bg-surface border border-border rounded-lg p-3 flex items-center gap-3">{children}</div>
  );
}

/** 리소스 행 삭제 버튼 — 클릭 시 onRemove 실행. */
export function DeleteButton({ onRemove }: { onRemove: () => void }) {
  return (
    <button className={deleteBtnCls} onClick={onRemove}>
      삭제
    </button>
  );
}
