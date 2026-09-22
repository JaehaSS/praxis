import type { ReactElement } from "react";
import type { MemoryReadiness } from "./memory-readiness";

interface Props {
  readiness: MemoryReadiness;
}

const TONE_CLASS: Record<MemoryReadiness["tone"], string> = {
  ready: "bg-status-done/15 text-status-done",
  needs_action: "bg-status-awaiting/15 text-status-awaiting",
  blocked: "bg-status-failed/15 text-status-failed",
  inactive: "bg-text-muted/15 text-text-muted",
  unknown: "bg-text-muted/15 text-text-secondary",
};

export function MemoryReadinessNote({ readiness }: Props): ReactElement {
  return (
    <div aria-label="주입 준비 상태" className="mt-1.5 flex items-start gap-2 text-[11px]">
      <span className={`shrink-0 rounded px-1.5 py-0.5 ${TONE_CLASS[readiness.tone]}`}>
        {readiness.label}
      </span>
      <span className="leading-relaxed text-text-muted">{readiness.detail}</span>
    </div>
  );
}
