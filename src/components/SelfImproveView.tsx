import { useCallback, useEffect, useState, type ReactElement } from "react";
import {
  proposalList,
  proposalApply,
  proposalReject,
  proposalWithdraw,
  captureEnabledGet,
  captureEnabledSet,
  type Proposal,
} from "../lib/ipc";

interface SelfImproveViewProps {
  onOpenMemoryCandidates?: () => Promise<void> | void;
}

/** S-07 Self-Improvement Review — 반성 제안을 지식 후보로 보내거나 거부하고, 적용을 되무른다. */
export function SelfImproveView({
  onOpenMemoryCandidates,
}: SelfImproveViewProps = {}): ReactElement {
  const [props, setProps] = useState<Proposal[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [captureOn, setCaptureOn] = useState<boolean | null>(null);

  const refresh = useCallback(async () => {
    try {
      // 철회 대상(적용됨)까지 보여야 하므로 전체를 받아 화면에서 가른다.
      setProps(await proposalList(false));
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    captureEnabledGet().then(setCaptureOn).catch(() => setCaptureOn(false));
  }, [refresh]);

  const toggleCapture = async () => {
    const next = !captureOn;
    try {
      await captureEnabledSet(next);
      setCaptureOn(next);
    } catch (e) {
      setErr(String(e));
    }
  };

  const decide = async (fn: () => Promise<unknown>) => {
    setErr(null);
    try {
      await fn();
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const createMemoryCandidate = async (proposalId: number): Promise<void> => {
    setErr(null);
    try {
      await proposalApply(proposalId);
      if (onOpenMemoryCandidates) {
        await onOpenMemoryCandidates();
        return;
      }
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const pending = props.filter((p) => p.status === "proposed");
  // 링크가 없는 옛 적용분은 무엇을 되무를지 알 수 없어 철회 대상이 아니다.
  const applied = props.filter((p) => p.status === "applied" && p.applied_memory_id !== null);

  return (
    <div className="flex-1 overflow-auto p-4">
      {err && <div className="text-status-failed text-sm font-code mb-2">{err}</div>}
      <div className="max-w-3xl mx-auto">
        <div className="flex items-center gap-3 mb-3 bg-surface border border-border rounded-lg px-3 py-2">
          <span className="text-sm">자동 캡처·회고</span>
          <button
            className={`ml-auto h-7 px-3 rounded-md text-sm font-medium ${
              captureOn ? "bg-primary text-bg" : "bg-border text-text-secondary"
            }`}
            onClick={toggleCapture}
          >
            {captureOn === null ? "…" : captureOn ? "ON" : "OFF"}
          </button>
        </div>
        <div className="text-text-muted text-xs leading-relaxed mb-3">
          ON이면 작업 종료 시(대화 작업은 변경이 있을 때) 메모리 후보 추출과 회고 제안을 위해
          Claude를 최대 두 번 호출할 수 있습니다. 호출 비용이 들며 현재 작업에는 소급 적용되지
          않습니다.
        </div>
        <div className="text-text-muted text-xs mb-3">
          자기개선은 모델 학습이 아니라 회고 제안 검토함입니다. 제안을 후보로 만든 뒤에도 근거 확인과
          사람 승인 전에는 새 작업에 적용되지 않습니다.
        </div>
        {pending.length === 0 ? (
          <div className="text-text-muted text-center py-12">
            검토할 제안이 없습니다.<br />
            작업을 완료하거나 활동 패널에서 &ldquo;이 작업 회고하기&rdquo;를 누르면 제안이 올라옵니다.
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {pending.map((p) => (
              <div key={p.id} className="bg-surface border border-border rounded-lg p-3">
                <div className="flex items-center gap-2 mb-2">
                  <span className="text-xs font-medium text-status-awaiting">{p.kind}</span>
                  <span className="text-text-muted text-xs font-code truncate">{p.repo}</span>
                </div>
                <div className="text-md mb-3 break-words">{p.content}</div>
                <div className="flex gap-2">
                  <button
                    className="h-8 px-3 rounded-md bg-primary text-bg text-sm font-medium"
                    onClick={() => void createMemoryCandidate(p.id)}
                  >
                    메모리 후보 만들기
                  </button>
                  <button
                    className="h-8 px-3 rounded-md text-text-secondary hover:bg-border text-sm"
                    onClick={() => decide(() => proposalReject(p.id))}
                  >
                    거절
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        {applied.length > 0 && (
          <section className="mt-6">
            <h2 className="text-sm font-medium text-text-secondary mb-1">
              메모리 후보로 만든 제안
            </h2>
            <div className="text-text-muted text-xs mb-3">
              철회하면 이 제안이 만든 후보 지식을 보관 처리합니다 — 본문과 증거는 남습니다.
            </div>
            <div className="flex flex-col gap-2">
              {applied.map((p) => (
                <div key={p.id} className="bg-surface border border-border rounded-lg p-3">
                  <div className="flex items-center gap-2 mb-2">
                    <span className="text-xs font-medium text-status-done">{p.kind}</span>
                    <span className="text-text-muted text-xs font-code truncate">{p.repo}</span>
                    <span className="text-text-muted text-xs ml-auto font-code">
                      지식 #{p.applied_memory_id}
                    </span>
                  </div>
                  <div className="text-md mb-3 break-words">{p.content}</div>
                  <div className="flex gap-2">
                    {onOpenMemoryCandidates && (
                      <button
                        className="h-8 px-3 rounded-md text-primary-bright hover:bg-border text-sm"
                        onClick={() => void onOpenMemoryCandidates()}
                      >
                        후보 목록 보기
                      </button>
                    )}
                    <button
                      className="h-8 px-3 rounded-md text-text-secondary hover:bg-border text-sm"
                      onClick={() => decide(() => proposalWithdraw(p.id))}
                    >
                      철회
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}
