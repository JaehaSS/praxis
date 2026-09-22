import { useEffect, useRef, type ReactNode } from "react";
import { DiffTab } from "./DiffTab";

interface Props {
  path: string | null;
  onBack: () => void;
  onOpenPath: (path: string) => void;
  children: ReactNode;
}

/** Keeps the session tree mounted while one selected diff takes its visual place. */
export function SessionDiffSurface({ path, onBack, onOpenPath, children }: Props) {
  const backRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (path == null) return;
    const frame = requestAnimationFrame(() => backRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [path]);

  return (
    <div className="relative flex min-h-0 min-w-0 flex-1 flex-col" data-session-diff-surface>
      <div
        className={`${path == null ? "" : "pointer-events-none invisible"} flex min-h-0 min-w-0 flex-1 flex-col`}
        inert={path != null}
      >
        {children}
      </div>
      {path != null && (
        <div className="absolute inset-0 z-10 flex min-h-0 min-w-0 flex-col bg-bg" data-central-diff>
          <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
            <button
              ref={backRef}
              className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-raised hover:text-text"
              onClick={onBack}
              aria-label="대화로 돌아가기"
            >
              ← 대화로 돌아가기
            </button>
            <span className="min-w-0 truncate font-code text-xs text-text-muted" title={path}>{path}</span>
          </div>
          <DiffTab path={path} active onOpenPath={onOpenPath} />
        </div>
      )}
    </div>
  );
}
