import { type ReactElement } from "react";
import { labelFor } from "../../lib/agents";
import type { DebateSpeaker } from "../../lib/ipc";
import type { ConvoItem } from "./ConversationView";
import type { DebateRound } from "./debate-rounds";
import { Icon } from "./icons";
import { Markdown } from "./Markdown";
import { MIN_DEBATE_PANE_WIDTH } from "./workspace-split-width";
import type { DebatePane } from "./DebateView";

export function PaneHeader({ pane, side, active }: { pane: DebatePane; side: DebateSpeaker; active: boolean }): ReactElement {
  const name = labelFor(pane.agent);
  return (
    <div
      className="flex items-center gap-2 border-b border-border px-3 py-1.5 text-xs"
      aria-label={`${side === "left" ? "좌측" : "우측"} 발화자 ${name}`}
    >
      <Icon name="sparkle" size={12} />
      <span className={active ? "text-text" : "text-text-secondary"}>{name}</span>
      {pane.model && <span className="rounded border border-border px-1.5 py-0.5 font-code text-[11px] text-text-muted">{pane.model}</span>}
      {/* 토론 중 세션 교체는 그쪽 벤더 맥락을 통째로 버린다 — 손잡이는 있되 눌리지 않는다. */}
      <button
        type="button"
        className="ml-auto text-text-muted disabled:cursor-not-allowed disabled:opacity-50"
        disabled
        title="토론 중에는 에이전트·모델을 바꿀 수 없습니다"
      >
        <Icon name="chevronDown" size={12} />
      </button>
    </div>
  );
}

export function Items({ items, onOpenLink }: { items: ConvoItem[]; onOpenLink?: (link: string) => void }): ReactElement {
  return (
    <div className="space-y-2 px-3 py-2">
      {items.map((item, i) => {
        if (item.role === "text") return <Markdown key={i} text={item.text} onOpenLink={onOpenLink} />;
        if (item.role === "user") return <div key={i} className="rounded-lg bg-primary/15 px-3 py-2 text-md text-text whitespace-pre-wrap">{item.text}</div>;
        if (item.role === "error") return <div key={i} className="font-code text-sm text-status-failed whitespace-pre-wrap">{item.text}</div>;
        if (item.role === "divider") return <div key={i} role="separator" className="py-1 text-[11px] text-text-muted">{item.text}</div>;
        if (item.role === "tool" || item.role === "tool_result")
          return (
            <div key={i} className="truncate rounded-md border border-border px-2 py-1 font-code text-xs text-text-muted">
              {item.role === "tool" ? `${item.name} ${item.summary}` : item.summary}
            </div>
          );
        return null;
      })}
    </div>
  );
}

export function Round({ round, cap, active, busy, onOpenLink }: {
  round: DebateRound;
  cap: number;
  active: DebateSpeaker;
  busy: boolean;
  onOpenLink?: (link: string) => void;
}): ReactElement {
  const cursor = (side: DebateSpeaker) =>
    busy && active === side ? <div className="px-3 pb-2 text-xs text-primary-bright">▌ 응답 생성 중…</div> : null;
  return (
    <div>
      {/* 구분선은 두 면을 가로지른다 — 면마다 그으면 어느 발화가 어느 발화의 답인지 읽을 수 없다. */}
      <div className="flex items-center gap-2 border-y border-border bg-raised px-3 py-1" role="heading" aria-level={3}>
        <span className="h-px flex-1 bg-border" />
        <span className="text-[11px] text-text-secondary">라운드 {round.no}/{cap}</span>
        <span className="h-px flex-1 bg-border" />
      </div>
      {round.user && <Items items={[{ role: "user", text: round.user }]} onOpenLink={onOpenLink} />}
      {round.common.length > 0 && <Items items={round.common} onOpenLink={onOpenLink} />}
      <div className="grid grid-cols-2 divide-x divide-border">
        <div style={{ minWidth: MIN_DEBATE_PANE_WIDTH }}>
          <Items items={round.left} onOpenLink={onOpenLink} />
          {cursor("left")}
        </div>
        <div style={{ minWidth: MIN_DEBATE_PANE_WIDTH }}>
          <Items items={round.right} onOpenLink={onOpenLink} />
          {cursor("right")}
        </div>
      </div>
    </div>
  );
}

