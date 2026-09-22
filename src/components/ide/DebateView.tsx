import { useMemo, useState, type ReactElement } from "react";
import { labelFor } from "../../lib/agents";
import { convoInterrupt, debateEnd, type DebateEndReason } from "../../lib/ipc";
import { Items, PaneHeader, Round } from "./DebateRoundGrid";
import { activeSide, consensusText, debateRounds, type DebateEventLike } from "./debate-rounds";
import { MIN_DEBATE_SESSION_WIDTH } from "./workspace-split-width";

/** 한 면이 누구인지 — 값은 좌측이 `tasks`, 우측이 `convo_debate_sides`에서 온다. */
export interface DebatePane {
  agent: string;
  model: string | null;
}

interface Props {
  taskId: number;
  events: readonly DebateEventLike[];
  /** 설정의 라운드 상한 — 헤더의 `라운드 n/N` 분모. */
  roundCap: number;
  left: DebatePane;
  right: DebatePane;
  busy: boolean;
  /** 받아들여졌으면 true — 거절된 발화의 초안은 지우지 않는다. */
  onSend: (text: string) => boolean | Promise<boolean>;
  /** `토론 끝내기` 성공 — App이 단일 뷰로 돌아간다. */
  onEnded: () => void;
  onOpenLink?: (link: string) => void;
}

/** 배너 문구 — 상태는 `ended(reason)` 하나이고 이유마다 다른 것은 문자열뿐이다(설계 0020 §5). */
const BANNER: Record<DebateEndReason, string> = {
  consensus: "합의했습니다",
  round_cap: "합의하지 못했습니다 — 무엇이 남았는지 각 면 마지막 줄을 보세요",
  aborted: "토론을 중단했습니다 — 지금까지 발화는 남습니다",
  error: "턴이 실패해 토론을 멈췄습니다",
};

/**
 * 토론 세션 — 라운드를 행으로 하는 grid 둘과 전폭 컴포저 하나.
 *
 * 면은 정확히 둘이고 자리는 발화자가 소유한다. 입력창이 하나이므로 포커스도 하나다 —
 * 에디터 분할에서 빌릴 것이 없는 이유가 이것이다(설계 0020 §5).
 */
export function DebateView({ taskId, events, roundCap, left, right, busy, onSend, onEnded, onOpenLink }: Props): ReactElement {
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const transcript = useMemo(() => debateRounds(events), [events]);
  const active = activeSide(transcript);
  const ended = transcript.ended;
  const activeName = labelFor(active === "left" ? left.agent : right.agent);

  const send = async () => {
    const text = draft.trim();
    if (!text || busy) return;
    // 세션이 바쁘거나 끊겨 있으면 보내는 쪽이 조용히 되돌아온다 — 그때 초안을 지우면 글이 사라진다.
    if (await onSend(text)) setDraft("");
  };

  const copyConclusion = () => {
    const text = consensusText(transcript);
    if (!text) return setError("복사할 결론이 없습니다");
    void navigator.clipboard?.writeText(text).catch((cause) => setError(String(cause)));
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1 overflow-auto">
        <div style={{ minWidth: MIN_DEBATE_SESSION_WIDTH }}>
          <div className="sticky top-0 z-10 grid grid-cols-2 divide-x divide-border bg-raised">
            <PaneHeader pane={left} side="left" active={busy && active === "left"} />
            <PaneHeader pane={right} side="right" active={busy && active === "right"} />
          </div>
          {transcript.preamble.length > 0 && (
            <>
              <div className="px-3 pt-2 text-[11px] text-text-muted">토론 이전 대화 (발화자 미상)</div>
              <Items items={transcript.preamble} onOpenLink={onOpenLink} />
            </>
          )}
          {transcript.rounds.map((round, i) => (
            <Round key={i} round={round} cap={roundCap} active={active} busy={busy} onOpenLink={onOpenLink} />
          ))}
        </div>
      </div>

      {/* 진행은 턴 시작·종료 두 번만 읽힌다 — 토큰마다 읽으면 아무것도 들리지 않는다. */}
      <div className="sr-only" aria-live="polite">
        {busy ? `${activeName} 차례입니다` : ended ? `토론이 끝났습니다 — ${BANNER[ended]}` : ""}
      </div>

      {ended && (
        <div className="flex flex-wrap items-center gap-2 border-t border-border bg-raised px-3 py-2 text-sm">
          <span className={ended === "consensus" ? "text-primary-bright" : "text-text-secondary"}>
            {ended === "consensus" ? "✓ " : ""}
            {BANNER[ended]}
          </span>
          {ended === "consensus" && (
            <button
              type="button"
              className="rounded-md border border-border px-2 py-1 text-xs text-text-secondary hover:border-border-strong"
              onClick={copyConclusion}
            >
              결론 복사
            </button>
          )}
          <button
            type="button"
            className="ml-auto rounded-md border border-border px-2 py-1 text-xs text-text-secondary hover:border-border-strong"
            onClick={() => void debateEnd(taskId).then(onEnded, (cause) => setError(String(cause)))}
          >
            토론 끝내기
          </button>
        </div>
      )}
      {error && <div className="border-t border-border px-3 py-1 text-xs text-status-failed">{error}</div>}

      <div className="flex items-end gap-2 border-t border-border px-3 py-2">
        <textarea
          className="min-h-[38px] flex-1 resize-none rounded-md border border-border bg-bg px-2 py-1.5 text-sm text-text outline-none focus:border-primary disabled:opacity-60"
          rows={1}
          value={draft}
          disabled={busy}
          placeholder={busy ? "라운드가 도는 동안 입력할 수 없습니다 — 중단하면 지금까지 발화는 남습니다" : "이어서 물어보세요…"}
          aria-label="토론 입력"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void send();
            }
          }}
        />
        {busy ? (
          <button
            type="button"
            className="rounded-md border border-dangerborder px-2 py-1.5 text-xs text-status-failed hover:border-status-failed"
            onClick={() => void convoInterrupt(taskId).catch((cause) => setError(String(cause)))}
          >
            중단
          </button>
        ) : (
          <button
            type="button"
            className="rounded-md border border-primary/50 px-2 py-1.5 text-xs text-primary-bright hover:border-primary disabled:border-border disabled:text-text-muted"
            disabled={!draft.trim()}
            onClick={() => void send()}
          >
            전송
          </button>
        )}
      </div>
    </div>
  );
}
