import { useCallback, useEffect, useRef, useState, type ReactElement } from "react";
import { useHostScope } from "../lib/host-scope";
import type { HostId } from "../lib/transport";
import {
  memoryAddCodeEvidence,
  memoryAddExternalDocumentEvidence,
  memoryAddLocalDocumentEvidence,
  memoryEvidence,
  memoryRevalidate,
  type Memory,
  type MemoryEvidence,
} from "../lib/ipc";
import {
  evidenceActionErrorForMemory,
  evidenceCountLabel,
  evidenceLocatorLabel,
  evidenceRequestBelongsToMemory,
  evidenceStateForMemory,
  evidenceStatusLabel,
  evidenceStatusTone,
  evidenceTimestampLabel,
  type EvidenceActionError,
  type EvidenceLoadState,
} from "./memory-evidence";

interface Props {
  memory: Memory;
  onChanged: () => Promise<void>;
}

async function addCode(host: HostId, memoryId: number): Promise<void> {
  const relativePath = window.prompt("레포 기준 코드 경로", "src/lib.rs")?.trim();
  if (!relativePath) return;
  const lineStart = Number(window.prompt("시작 줄 (1부터)", "1"));
  const lineEnd = Number(window.prompt("끝 줄 (포함)", String(lineStart)));
  if (!Number.isInteger(lineStart) || !Number.isInteger(lineEnd)) {
    throw new Error("줄 번호는 정수여야 합니다");
  }
  await memoryAddCodeEvidence(host, memoryId, {
    relative_path: relativePath,
    line_start: lineStart,
    line_end: lineEnd,
  });
}

async function addLocalDocument(host: HostId, memoryId: number): Promise<void> {
  const relativePath = window.prompt("레포 기준 문서 경로", "docs/README.md")?.trim();
  if (!relativePath) return;
  await memoryAddLocalDocumentEvidence(host, memoryId, {
    relative_path: relativePath,
    expires_at: null,
  });
}

async function addExternalDocument(host: HostId, memoryId: number): Promise<void> {
  const url = window.prompt("HTTPS 문서 URL (query/fragment 제외)")?.trim();
  if (!url) return;
  const days = Number(window.prompt("유효 기간(일)", "7"));
  if (!Number.isFinite(days) || days <= 0) throw new Error("유효 기간은 양수여야 합니다");
  await memoryAddExternalDocumentEvidence(host, memoryId, {
    url,
    expires_at: Math.floor(Date.now() / 1000) + Math.floor(days * 86400),
  });
}

function EvidenceRows({ rows }: { rows: MemoryEvidence[] }): ReactElement {
  if (rows.length === 0) {
    return <div className="text-text-muted">근거가 없습니다. 확인·승인 또는 backend 관측 근거를 추가하세요.</div>;
  }
  return (
    <div className="flex flex-col gap-1">
      {rows.map((row) => (
        <div key={row.id} className="font-code">
          <div className="flex gap-2">
            <span className={evidenceStatusTone(row.status)}>{evidenceStatusLabel(row.status)}</span>
            <span className="text-text-muted">{row.kind}</span>
            <span className="text-text-secondary truncate" title={evidenceLocatorLabel(row)}>
              {evidenceLocatorLabel(row)}
            </span>
          </div>
          <div className="text-text-muted ml-2">
            검사 {evidenceTimestampLabel(row.checked_at)}
            {row.expires_at !== null && ` · 만료 ${evidenceTimestampLabel(row.expires_at)}`}
          </div>
        </div>
      ))}
    </div>
  );
}

export function EvidenceLoadStateView({
  state,
  onRetry,
}: {
  state: EvidenceLoadState;
  onRetry: () => void;
}): ReactElement {
  if (state.status === "loading") {
    return <div className="text-text-muted">근거를 불러오는 중…</div>;
  }
  if (state.status === "error") {
    return (
      <div className="rounded border border-status-failed/40 bg-status-failed/5 p-2">
        <div className="text-status-failed">근거를 불러오지 못했습니다 · {state.message}</div>
        <button className="mt-1 text-primary-bright hover:text-text" onClick={onRetry}>
          다시 시도
        </button>
      </div>
    );
  }
  return <EvidenceRows rows={state.rows} />;
}

function useEvidenceLoader(host: HostId, memoryId: number): {
  state: EvidenceLoadState;
  reload: () => Promise<void>;
} {
  const requestRef = useRef(0);
  const activeMemoryRef = useRef<number | null>(memoryId);
  const [state, setState] = useState<EvidenceLoadState>({
    memoryId,
    status: "loading",
  });
  const reload = useCallback(async (): Promise<void> => {
    if (!evidenceRequestBelongsToMemory(activeMemoryRef.current, memoryId)) return;
    const request = requestRef.current + 1;
    requestRef.current = request;
    setState({ memoryId, status: "loading" });
    try {
      const rows = await memoryEvidence(host, memoryId);
      if (requestRef.current === request) {
        setState({ memoryId, status: "ready", rows });
      }
    } catch (reason) {
      if (requestRef.current === request) {
        setState({ memoryId, status: "error", message: String(reason) });
      }
    }
  }, [memoryId]);
  useEffect(() => {
    activeMemoryRef.current = memoryId;
    void reload();
    return () => {
      activeMemoryRef.current = null;
      requestRef.current += 1;
    };
  }, [memoryId, reload]);
  return { state: evidenceStateForMemory(state, memoryId), reload };
}

export function MemoryEvidencePanel({ memory, onChanged }: Props): ReactElement {
  // 메모리는 그것을 보관한 머신의 DB에 산다 — 스코프가 그 머신을 정한다 (ADR 0133).
  const host = useHostScope();
  const { state, reload } = useEvidenceLoader(host, memory.id);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<EvidenceActionError | null>(null);

  const run = async (action: () => Promise<unknown>): Promise<void> => {
    setBusy(true);
    setActionError(null);
    try {
      await action();
      await reload();
      await onChanged();
    } catch (reason) {
      setActionError({ memoryId: memory.id, message: String(reason) });
    } finally {
      setBusy(false);
    }
  };
  const loading = state.status === "loading";
  const count = evidenceCountLabel(state);
  const visibleActionError = evidenceActionErrorForMemory(actionError, memory.id);

  return (
    <div className="mt-2 ml-7 border-t border-border pt-2 text-xs">
      <div className="flex items-center gap-2 mb-2">
        <span className="text-text-muted mr-auto">현재 버전 근거 {count}</span>
        {memory.tier === "project" && (
          <>
            <button disabled={busy || loading} onClick={() => void run(() => addCode(host, memory.id))}>
              코드 추가
            </button>
            <button disabled={busy || loading} onClick={() => void run(() => addLocalDocument(host, memory.id))}>
              문서 추가
            </button>
          </>
        )}
        <button disabled={busy || loading} onClick={() => void run(() => addExternalDocument(host, memory.id))}>
          외부 문서
        </button>
        <button disabled={busy || loading} onClick={() => void run(() => memoryRevalidate(host, memory.id))}>
          재검증
        </button>
      </div>
      {visibleActionError && (
        <div className="text-status-failed mb-1">{visibleActionError}</div>
      )}
      <EvidenceLoadStateView state={state} onRetry={() => void reload()} />
    </div>
  );
}
