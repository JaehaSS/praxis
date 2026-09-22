import type { VerifyReport } from "../../lib/ipc";
import { Icon } from "./icons";

interface Props {
  report: VerifyReport;
  onClose: () => void;
}

/** 검증 결과 플로팅 패널 — 사용 명령 + build/test exit + 요약 + ready + 경고. */
export function EvidencePanel({ report, onClose }: Props) {
  const { spec, build, test, summary, ready, warnings } = report;
  const exitMark = (c: { exit_code: number } | null) =>
    c == null ? null : c.exit_code === 0 ? (
      <span className="text-status-done">✓ exit 0</span>
    ) : (
      <span className="text-status-failed">✗ exit {c.exit_code}</span>
    );

  return (
    <div className="absolute right-4 bottom-20 z-30 w-[28rem] max-h-72 overflow-auto rounded-lg border border-border-strong bg-raised shadow-xl p-3">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs uppercase tracking-wide text-text-muted flex items-center gap-1">
          검증 결과
          <span
            className={`ml-1 px-1.5 py-0.5 rounded text-[11px] ${
              ready ? "bg-addbg text-status-done" : "bg-dangerbg text-status-failed"
            }`}
          >
            {ready ? "READY" : "NOT READY"}
          </span>
        </span>
        <button className="text-text-muted hover:text-text" onClick={onClose} aria-label="닫기">
          <Icon name="x" size={14} />
        </button>
      </div>

      <div className="text-sm flex flex-col gap-1.5">
        <div className="flex items-center justify-between gap-2">
          <span className="font-code text-xs text-text-secondary truncate" title={spec.build ?? ""}>
            build: {spec.build ?? "—"}
          </span>
          {exitMark(build)}
        </div>
        <div className="flex items-center justify-between gap-2">
          <span className="font-code text-xs text-text-secondary truncate" title={spec.test ?? ""}>
            test: {spec.test ?? "—"}
          </span>
          <span className="flex items-center gap-2 shrink-0">
            {summary && (
              <span className="text-xs">
                <span className="text-status-done">{summary.passed} passed</span>
                {summary.failed > 0 && (
                  <span className="text-status-failed"> · {summary.failed} failed</span>
                )}
              </span>
            )}
            {exitMark(test)}
          </span>
        </div>
      </div>

      {warnings.length > 0 && (
        <div className="mt-2 pt-2 border-t border-border flex flex-col gap-1">
          {warnings.map((w, i) => (
            <div key={i} className="text-xs text-status-awaiting">
              ⚠ {w}
            </div>
          ))}
        </div>
      )}

      {(build || test) && (
        <details className="mt-2 pt-2 border-t border-border">
          <summary className="text-xs text-text-muted cursor-pointer">출력 보기</summary>
          <pre className="mt-1 text-[11px] font-code text-text-secondary whitespace-pre-wrap max-h-32 overflow-auto">
            {[build?.tail, test?.tail].filter(Boolean).join("\n---\n")}
          </pre>
        </details>
      )}
    </div>
  );
}
