import { useState } from "react";
import type { TaskRef } from "../../lib/transport";
import { contextFileRead, type ContextFile, type ContextReport } from "../../lib/ipc";
import { ago, outcomeBadge as outcome } from "../../lib/fmt";
import { Icon } from "./icons";
import { memoryEmptyState, memoryReceiptLabel } from "./memory-inspector";

interface Props {
  task: TaskRef;
  report: ContextReport;
  selectedMemoryId?: number;
  onClose: () => void;
}

const roleLabel: Record<ContextFile["role"], string> = {
  global: "모든 레포 공통",
  project: "이 프로젝트",
};

const fmtSize = (n: number) => (n >= 1024 ? `${(n / 1024).toFixed(1)}KB` : `${n}B`);

/** 파일 1건 — 존재/크기/PRAXIS 블록 뱃지 + 클릭 시 내용 지연 로드. */
function ContextFileRow({ task, file }: { task: TaskRef; file: ContextFile }) {
  const [open, setOpen] = useState(false);
  const [content, setContent] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const toggle = async () => {
    if (!file.exists) return;
    const next = !open;
    setOpen(next);
    if (next && content === null && !loading) {
      setLoading(true);
      setErr(null);
      try {
        setContent(await contextFileRead(task, file.path));
      } catch (e) {
        setErr(String(e));
      } finally {
        setLoading(false);
      }
    }
  };

  return (
    <div className="text-xs">
      <button
        className={`w-full flex items-center gap-2 py-1 text-left ${
          file.exists ? "hover:text-text" : "opacity-50 cursor-default"
        }`}
        onClick={toggle}
        disabled={!file.exists}
      >
        <span className={file.exists ? "text-status-done" : "text-text-muted"}>
          <Icon name={file.exists ? "check" : "x"} size={12} />
        </span>
        <span className="w-20 shrink-0 text-text-muted">{roleLabel[file.role]}</span>
        <span className="font-code text-text-secondary truncate flex-1" title={file.path}>
          {file.path}
        </span>
        {file.exists && <span className="text-text-muted shrink-0">{fmtSize(file.size)}</span>}
        {file.has_praxis_block && (
          <span className="shrink-0 px-1.5 rounded bg-primary/15 text-primary-bright">PRAXIS</span>
        )}
      </button>
      {open && (
        <div className="ml-6 mb-1">
          {loading && <div className="text-text-muted">불러오는 중…</div>}
          {err && <div className="text-status-failed">{err}</div>}
          {content !== null && (
            <pre className="font-code text-text-secondary whitespace-pre-wrap bg-bg border border-border rounded p-2 max-h-40 overflow-auto">
              {content}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

/** 컨텍스트 가시성 패널(설계 0008 §A) — 벤더 4종 × {글로벌, 프로젝트} 실측 + 이 작업의 주입 이력.
 *  "메모리가 실제로 어떤 파일로 이 세션에 들어갔는가"를 벤더별로 정직하게 보여준다(병합 재현은 안 함). */
export function MemoryInspectorPanel({ task, report, selectedMemoryId, onClose }: Props) {
  const [openVendor, setOpenVendor] = useState<string | null>(report.vendors[0]?.vendor ?? null);
  const emptyState = memoryEmptyState(report);

  return (
    <div className="absolute right-4 bottom-20 z-30 w-[32rem] max-h-[28rem] overflow-auto rounded-lg border border-border-strong bg-raised shadow-xl p-3">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs uppercase tracking-wide text-text-muted">컨텍스트 확인</span>
        <button className="text-text-muted hover:text-text" onClick={onClose} aria-label="닫기">
          <Icon name="x" size={14} />
        </button>
      </div>

      <div className="text-[11px] text-text-muted mb-2">
        벤더가 읽는 컨텍스트 파일을 실측한 결과입니다 — 벤더별 병합 규칙(우선순위·상위 디렉터리 탐색)은 재현하지
        않습니다.
      </div>

      <div className="flex flex-col gap-1 mb-3">
        {report.vendors.map((v) => (
          <div key={v.vendor} className="border border-border rounded">
            <button
              className="w-full flex items-center gap-2 px-2 py-1.5 text-left text-sm hover:bg-bg"
              onClick={() => setOpenVendor((cur) => (cur === v.vendor ? null : v.vendor))}
            >
              <span className="font-medium flex-1">{v.vendor}</span>
              {v.uncertain && (
                <span className="text-[10px] px-1.5 rounded bg-status-awaiting/15 text-status-awaiting">
                  경로 불확실
                </span>
              )}
              <span className="text-text-muted">
                <Icon name={openVendor === v.vendor ? "chevronDown" : "chevronRight"} size={12} />
              </span>
            </button>
            {openVendor === v.vendor && (
              <div className="px-2 pb-2 border-t border-border">
                {v.files.map((f) => (
                  <ContextFileRow key={`${v.vendor}-${f.role}`} task={task} file={f} />
                ))}
              </div>
            )}
          </div>
        ))}
      </div>

      {/* 주입 이력 — 빈 상태는 원인별로 분리 표시 */}
      <div className="text-xs text-text-muted mb-1">
        {selectedMemoryId == null ? `적용된 메모리 ${report.injected.length}건` : `선택 메모리 #${selectedMemoryId} 전달 이력`}
      </div>
      {selectedMemoryId == null && emptyState ? (
        <div className="mb-2 rounded border border-border bg-bg px-2 py-1.5 text-xs">
          <div className="text-text-secondary">{emptyState.title}</div>
          <div className="mt-0.5 text-text-muted">{emptyState.detail}</div>
          {!report.capture_enabled && (
            <div className="mt-1 text-[11px] text-text-muted">
              자동 캡처 OFF · 새 후보 생성만 멈추며 이 작업의 주입 0건 원인은 아닙니다.
            </div>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-1">
          {report.injected.filter((m) => selectedMemoryId == null || m.memory_id === selectedMemoryId).map((m, index) => {
            const b = outcome(m.outcome);
            return (
              <div
                key={`${m.memory_id}-${m.version ?? "legacy"}-${m.injected_at}-${index}`}
                className="flex items-start gap-2 text-xs"
              >
                <span className={`w-12 shrink-0 ${b.c}`}>{b.t}</span>
                <span className="w-16 shrink-0 text-text-muted">{m.kind ?? "—"}</span>
                <span className="flex-1 min-w-0">
                  <span
                    className={`block break-words ${m.exists ? "text-text-secondary" : "text-text-muted line-through"}`}
                  >
                    {m.version === null ? "(과거 원문 미확인)" : (m.content ?? "(삭제된 메모리)")}
                  </span>
                  <span
                    className="block font-code text-[10px] text-text-muted truncate"
                    title={`${memoryReceiptLabel(m)} · ${m.target_paths.join(", ")}`}
                  >
                    {m.version === null ? "과거 사용 기록 · 버전 미확인" : memoryReceiptLabel(m)}
                  </span>
                </span>
                <span className="text-text-muted font-code shrink-0">{ago(m.injected_at)}</span>
              </div>
            );
          })}
        </div>
      )}
      {selectedMemoryId != null && !report.injected.some((m) => m.memory_id === selectedMemoryId) && (
        <div className="text-xs text-text-muted">선택 메모리의 전달 기록 확인 불가</div>
      )}
    </div>
  );
}
