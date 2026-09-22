import { useCallback, useEffect, useRef, useState } from "react";
import { taskApprovalStatus } from "../../lib/ipc";
import { taskKey, type TaskRef } from "../../lib/transport";
import { approvalRepairPrompt, approvalStageLabel, readinessSummary, type ApprovalStatus } from "../../lib/approval-readiness";
import { DetailChip } from "./DetailChip";

interface Props {
  task: TaskRef;
  base: string;
  refreshKey: string;
  disabled: boolean;
  onRepair: (message: string) => Promise<boolean>;
  onResolve?: () => void;
}

export function ApprovalReadinessPanel({ task, base, refreshKey, disabled, onRepair, onResolve }: Props) {
  const key = taskKey(task);
  const generation = useRef(0);
  const lifecycle = useRef(0);
  const sendInFlight = useRef(false);
  const [status, setStatus] = useState<ApprovalStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [sending, setSending] = useState(false);
  const [sent, setSent] = useState(false);
  const [open, setOpen] = useState(false);

  const reload = useCallback(async () => {
    const request = ++generation.current;
    setLoading(true);
    setError(null);
    try {
      const value = await taskApprovalStatus({ host: task.host, id: task.id });
      if (generation.current === request) setStatus(value);
    } catch (reason) {
      if (generation.current === request) { setStatus(null); setError(String(reason)); }
    } finally {
      if (generation.current === request) setLoading(false);
    }
  }, [task.host, task.id]);

  useEffect(() => {
    lifecycle.current++;
    sendInFlight.current = false;
    setStatus(null); setSent(false); setSending(false);
    void reload();
    const focus = () => { void reload(); };
    window.addEventListener("focus", focus);
    return () => { generation.current++; lifecycle.current++; window.removeEventListener("focus", focus); };
  }, [key, refreshKey, reload]);

  // 상세는 작업이 바뀔 때만 접는다. 갱신(refreshKey)에 얹으면 안 된다 — 대화를 이어가는 동안
  // 메시지마다 updated_at이 바뀌므로, 펼쳤든 접었든 사용자의 선택이 매 턴 지워진다.
  useEffect(() => { setOpen(false); }, [key]);

  const repair = async () => {
    if (!status || sendInFlight.current || disabled) return;
    sendInFlight.current = true;
    const request = lifecycle.current;
    setSending(true); setError(null);
    try {
      const accepted = await onRepair(approvalRepairPrompt(task, base, status));
      if (lifecycle.current === request) {
        setSent(accepted);
        if (!accepted) setError("수정 요청을 보내지 못했습니다. 연결과 작업 상태를 확인한 뒤 다시 시도하세요.");
      }
    } catch (reason) {
      if (lifecycle.current === request) setError(String(reason));
    } finally {
      if (lifecycle.current === request) { setSending(false); sendInFlight.current = false; }
    }
  };

  const issues = status?.readiness?.issues ?? [];
  const latest = status?.attempts[0];
  const failed = latest?.outcome === "failed";
  const commitFailed = failed && ["commit", "git_commit_failed"].includes(latest.stage);
  const conflict = issues.some((issue) => ["merge_conflict", "source_merge"].includes(issue.code));
  const remote = status?.readiness;
  const remoteChanged = remote && ((remote.remote_ahead ?? 0) > 0 || (remote.remote_behind ?? 0) > 0);
  // 확인이 필요하다는 사실은 칩 색으로 말한다. 예전처럼 상세를 자동으로 펼치면 승인 바가
  // 그만큼 두꺼워져, 대화를 이어가려던 사용자가 매번 접어야 한다.
  const needsAttention = issues.length > 0 || failed || Boolean(status?.inspection_error);
  const summary = loading ? "승인 준비 확인 중…" : status ? readinessSummary(status) : "준비 상태 확인 불가";

  return (
    <DetailChip label="승인 준비" summary={summary} open={open} onOpenChange={setOpen} attention={needsAttention}>
      <div className="flex flex-wrap items-center gap-2">
        <span className="min-w-0 flex-1 text-text-muted">점검 내용{status?.attempts.length ? ` · 최근 시도 ${status.attempts.length}건` : ""}</span>
        <button type="button" disabled={loading || disabled || sending} onClick={() => void reload()} className="text-text-muted hover:text-text disabled:opacity-40">다시 확인</button>
      </div>
      {error && <p className="mt-1 whitespace-pre-wrap break-all text-text-muted">{error}</p>}
      {!status && !loading && <p className="mt-1 text-text-muted">점검을 지원하지 않는 호스트이거나 연결 오류일 수 있습니다. 승인 시 기존 검사는 그대로 실행됩니다.</p>}
      {status && (
        <div className="mt-2 space-y-2">
          {status.inspection_error && <p className="whitespace-pre-wrap break-all">{status.inspection_error}</p>}
          {remote && <p className="text-text-muted">대상: {remote.base} · {new Date(remote.observed_at * 1000).toLocaleTimeString()} 확인 · 미커밋 변경·훅·외부 편집은 승인 때 재검사합니다.</p>}
          {issues.map((issue, index) => (
            <div key={`${issue.code}:${index}`}>
              <p>{issue.message}</p>
              {issue.location && <p className="select-text break-all font-code text-text-muted">{issue.location}</p>}
              {!!issue.paths.length && <ul className="mt-1 max-h-32 overflow-auto font-code">{issue.paths.map((path) => <li key={path.path} className="whitespace-pre-wrap break-all">{path.status} {path.path}</li>)}</ul>}
              {issue.total_paths > issue.paths.length && <p>외 {issue.total_paths - issue.paths.length}건</p>}
            </div>
          ))}
          {remote && <p className="text-text-muted">{remote.remote_ahead == null ? "원격 추적 정보 없음" : `저장된 origin/${remote.base} 대비 로컬 ${remote.remote_ahead}커밋 앞 · ${remote.remote_behind}커밋 뒤`} · 원격 fetch는 실행하지 않았습니다.</p>}
          {failed && <p className="whitespace-pre-wrap break-all">최근 실패 단계: {approvalStageLabel(latest.stage)}{latest.error ? `\n${latest.error}` : ""}</p>}
          <div className="flex flex-wrap gap-3">
            {(failed || issues.length > 0 || remoteChanged || status.inspection_error) && <button type="button" disabled={disabled || sending || sent || loading} onClick={() => void repair()} className="text-primary-bright disabled:opacity-40">{sent ? "수정 요청 전달됨" : sending ? "수정 요청 전송 중…" : commitFailed ? "문서·커밋 오류 수정 요청" : "에이전트에 준비 문제 확인 요청"}</button>}
            {conflict && onResolve && <button type="button" disabled={disabled || sending || loading} onClick={onResolve} className="text-primary-bright disabled:opacity-40">충돌 확인·해소</button>}
          </div>
          {!!status.attempts.length && <ul className="space-y-1 text-text-muted" aria-label="승인 시도 이력">{status.attempts.map((attempt) => (
            <li key={attempt.attempt_id}>{new Date(attempt.ts * 1000).toLocaleString()} · {attempt.outcome === "succeeded" ? "완료" : attempt.outcome === "failed" ? "실패" : "진행 중 또는 결과 미기록"} · {approvalStageLabel(attempt.stage)}{attempt.direct ? " · 직접 실행" : ""}</li>
          ))}</ul>}
        </div>
      )}
    </DetailChip>
  );
}
