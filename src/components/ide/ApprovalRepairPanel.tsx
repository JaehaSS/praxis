import { useEffect, useRef, useState } from "react";
import { getTransport, taskKey, type TaskRef } from "../../lib/transport";
import type { ApprovalRepairSession } from "../../lib/approval-readiness";
import { DetailChip } from "./DetailChip";

const active = (session: ApprovalRepairSession | null) => session && ["running", "resolving", "checking"].includes(session.state);
const labels: Record<string, string> = { prepared: "해결 준비됨", running: "자동 해결 시작 중", resolving: "에이전트 해결 중", checking: "결과 검사 중", ready: "검사 통과 · 변경 검토 필요", needs_attention: "확인 필요", accepted: "해결 결과 채택됨 · 기존 승인으로 반영하세요" };

export function ApprovalRepairPanel({ task, disabled, onAccepted }: { task: TaskRef; disabled: boolean; onAccepted: () => void }) {
  const [session, setSession] = useState<ApprovalRepairSession | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [reviewed, setReviewed] = useState(false);
  const [stopping, setStopping] = useState(false);
  const generation = useRef(0);
  const inFlight = useRef(false);
  const version = useRef(0);
  const poll = useRef(false);
  const observed = useRef("");
  const key = taskKey(task);

  useEffect(() => {
    const current = ++generation.current;
    inFlight.current = false; poll.current = false; observed.current = ""; version.current++;
    setSession(null); setError(null); setBusy(false); setReviewed(false); setOpen(false); setStopping(false);
    const reload = async () => {
      const request = ++version.current;
      try {
        const result = await getTransport(task.host).approvalRepairStatus(task.id);
        if (generation.current === current && request === version.current) {
          const stamp = `${result?.id}:${result?.state}:${result?.updated_at}`;
          if (observed.current !== stamp) setReviewed(false);
          observed.current = stamp; poll.current = Boolean(active(result)); setSession(result);
        }
      } catch (reason) { if (generation.current === current && request === version.current) { setError(String(reason)); poll.current = false; } }
    };
    void reload();
    const focus = () => { void reload(); };
    window.addEventListener("focus", focus);
    const timer = window.setInterval(() => { if (poll.current && !document.hidden) void reload(); }, 4000);
    return () => { generation.current++; window.clearInterval(timer); window.removeEventListener("focus", focus); };
  }, [key, task.host, task.id]);

  const perform = async (kind: "prepare" | "run" | "accept") => {
    if (inFlight.current || disabled || (kind !== "prepare" && !session)) return;
    inFlight.current = true; version.current++; poll.current = kind === "run"; setStopping(false);
    setBusy(true); setOpen(true); setError(null); setReviewed(false);
    const current = generation.current;
    const transport = getTransport(task.host);
    try {
      const result = kind === "prepare" ? await transport.approvalRepairPrepare(task.id)
        : kind === "run" ? await transport.approvalRepairRun(task.id, session!.id)
        : await transport.approvalRepairAccept(task.id, session!.id);
      if (current === generation.current) { version.current++; poll.current = Boolean(active(result)); setSession(result); if (kind === "accept") onAccepted(); }
    } catch (reason) { if (current === generation.current) setError(String(reason)); }
    finally { if (current === generation.current) { setBusy(false); inFlight.current = false; } }
  };

  const cancel = async () => {
    if (!session || stopping) return;
    const current = generation.current; setStopping(true);
    try { await getTransport(task.host).approvalRepairCancel(task.id, session.id); }
    catch (reason) { if (generation.current === current) { setError(String(reason)); setStopping(false); } }
  };

  // 자동 해결은 드물게 쓰는 경로다 — 바에는 상태 한 줄만 남기고 준비·시작·채택은 팝오버 안에 둔다.
  const summary = session ? labels[session.state] ?? session.state : "자동 해결";

  return <DetailChip label="자동 해결" summary={summary} open={open} onOpenChange={setOpen} attention={Boolean(error) || session?.state === "needs_attention"}>
    <div className="flex flex-wrap items-center gap-3">
      <button type="button" disabled={disabled || busy || Boolean(active(session))} onClick={() => void perform("prepare")} className="text-primary-bright disabled:opacity-40">{session ? "새 자동 해결 준비" : "자동 해결 준비"}</button>
      {busy && <span role="status">처리 중…</span>}
      {active(session) && <button type="button" disabled={stopping} onClick={() => void cancel()}>{stopping ? "중단 요청됨" : "자동 해결 중단"}</button>}
    </div>
    {error && <p role="alert" className="mt-1 whitespace-pre-wrap break-all">{error}</p>}
    {session && <div className="mt-2 space-y-2">
      <p>후보 폴더에서 최대 2회 해결합니다. 기존 에이전트 사용량이 발생합니다. 원본 작업은 보관하고 대상 {session.base} 반영은 기존 승인에서 진행합니다.</p>
      <p className="break-all">원본 보관: {session.source_path}<br />해결 후보: {session.candidate_path}</p>
      <p>입력: {session.source_sha.slice(0, 8)} → 대상 {session.target_sha.slice(0, 8)} · 시도 {session.attempts}/2</p>
      <ul className="font-code">{session.commands.map((command, index) => <li key={index}>{command}</li>)}</ul>
      {!session.commands.length && <p>검사 명령이 없습니다. 프로젝트 검증 설정 후 다시 준비하세요.</p>}
      {session.state === "prepared" && <button type="button" disabled={disabled || busy || !session.commands.length} onClick={() => void perform("run")} className="text-primary-bright disabled:opacity-40">자동 해결·검사 시작</button>}
      {session.error && <pre className="whitespace-pre-wrap break-all">{session.error}</pre>}
      {!!session.summary && <details><summary>에이전트 설명</summary><pre className="whitespace-pre-wrap break-all">{session.summary}</pre></details>}
      {!!session.diff && <details open><summary>해결 변경</summary><pre className="max-h-52 overflow-auto whitespace-pre-wrap break-all font-code">{session.diff}</pre></details>}
      {session.checks.map((check, index) => <details key={index}><summary>{check.exit_code === 0 ? "통과" : "실패"} · {check.command}</summary><pre className="whitespace-pre-wrap break-all">{check.tail}</pre></details>)}
      {session.state === "ready" && <><label className="flex gap-2"><input type="checkbox" checked={reviewed} onChange={(event) => setReviewed(event.target.checked)} />양쪽 변경 의도와 검사 결과를 확인했습니다</label><button type="button" disabled={disabled || busy || !reviewed} onClick={() => void perform("accept")} className="text-primary-bright disabled:opacity-40">해결 결과 채택</button></>}
      {session.state === "accepted" && <p>원본은 위 경로에 보관돼 있습니다. 승인 전에 최종 diff를 확인하세요. 검증 실패 시 승인 차단 설정을 쓰면 새 후보에서 Verify를 실행하세요.</p>}
    </div>}
  </DetailChip>;
}
