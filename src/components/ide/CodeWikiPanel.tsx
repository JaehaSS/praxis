import type { CodeWikiModule, CodeWikiPageState, CodeWikiStatus } from "../../lib/code-wiki-ipc";

const PAGE_LABEL: Record<CodeWikiPageState, string> = {
  missing: "없음",
  ready: "최신",
  stale: "오래됨",
  conflict: "충돌",
  orphaned: "소스 삭제됨",
};

interface ControlProps {
  onOpen: () => void;
}

export function CodeWikiControl({ onOpen }: ControlProps) {
  return (
    <button
      className="h-6 rounded border border-border px-1.5 text-xs text-text-secondary hover:text-text"
      onClick={onOpen}
      aria-label="코드 Wiki 열기"
    >
      코드 Wiki
    </button>
  );
}

interface PanelProps {
  status: CodeWikiStatus | null;
  error: string | null;
  busy: boolean;
  sourcePath: string;
  dirty: boolean;
  onGenerate: (path: string | null) => void;
  onOpenPath: (path: string) => void;
  onReload: () => void;
  onClose: () => void;
}

function graphGuidance(graphState: string | undefined): string | null {
  if (graphState === "ready") return null;
  if (graphState === "stale") return "코드 그래프를 새로 고친 뒤 Wiki를 갱신하세요.";
  return "먼저 코드 그래프를 만들거나 새로 고치세요.";
}

function conflictMessage(index: CodeWikiPageState | undefined, current: CodeWikiModule | undefined) {
  if (index === "conflict" || current?.state === "conflict") {
    return "충돌한 Wiki 문서는 보존됩니다. 내용을 다른 경로에 보관한 뒤 충돌을 해소하세요.";
  }
  return null;
}

export function CodeWikiPanel({
  status,
  error,
  busy,
  sourcePath,
  dirty,
  onGenerate,
  onOpenPath,
  onReload,
  onClose,
}: PanelProps) {
  const current = status?.modules.find((module) => module.sourcePath === sourcePath);
  const guidance = graphGuidance(status?.graphState);
  const conflict = conflictMessage(status?.indexState, current);
  const writesDisabled = busy || dirty || guidance != null;
  return (
    <aside className="absolute right-2 top-10 z-30 w-96 max-w-[calc(100%-1rem)] rounded-lg border border-border-strong bg-raised p-3 shadow-xl">
      <div className="mb-2 flex items-center gap-2">
        <h2 className="text-sm text-text">코드 Wiki</h2>
        <button className="ml-auto text-text-muted hover:text-text" onClick={onClose} aria-label="코드 Wiki 닫기">×</button>
      </div>
      <div className="max-h-80 space-y-3 overflow-auto text-xs">
        <p className="text-text-muted" role="status">저장된 파일 기준으로 생성합니다. 저장하지 않은 변경은 포함되지 않습니다.</p>
        {guidance && <p className="text-status-awaiting">{guidance}</p>}
        {dirty && <p className="text-status-awaiting">저장하지 않은 소스 또는 Wiki 문서가 있어 갱신할 수 없습니다.</p>}
        {status?.detail && <p className="text-text-muted">{status.detail}</p>}
        {error && <p className="text-status-failed" role="alert">{error}</p>}
        {conflict && <p className="text-status-failed" role="alert">{conflict}</p>}
        <div className="space-y-1 text-text-muted">
          <p>목차 · {status ? PAGE_LABEL[status.indexState] : "확인 중"}</p>
          <p>현재 문서 · {current ? PAGE_LABEL[current.state] : "없음"}</p>
          <p className="truncate font-code">{status?.indexPath ?? "docs/codebase/index.md"}</p>
        </div>
        <div className="grid grid-cols-2 gap-2">
          <button className="rounded border border-border px-2 py-1 text-text hover:bg-bg disabled:opacity-40" disabled={writesDisabled} onClick={() => onGenerate(null)}>전체 생성</button>
          <button className="rounded border border-border px-2 py-1 text-text hover:bg-bg disabled:opacity-40" disabled={writesDisabled} onClick={() => onGenerate(sourcePath)}>현재 파일 갱신</button>
          <button className="rounded border border-border px-2 py-1 text-text hover:bg-bg disabled:opacity-40" disabled={busy || !status || status.indexState === "missing"} onClick={() => onOpenPath(status!.indexPath)}>목차 열기</button>
          <button className="rounded border border-border px-2 py-1 text-text hover:bg-bg disabled:opacity-40" disabled={busy || !current || current.state === "missing"} onClick={() => current && onOpenPath(current.pagePath)}>현재 문서 열기</button>
          <button className="col-span-2 rounded border border-border px-2 py-1 text-text hover:bg-bg disabled:opacity-40" disabled={busy} onClick={onReload}>상태 확인</button>
        </div>
      </div>
    </aside>
  );
}
