import { Icon } from "./icons";

interface Props {
  canSend: boolean;
  onInterrupt: () => void;
  onSend: () => void;
  sendLabel?: string;
}

export function AgentComposerActions({ canSend, onInterrupt, onSend, sendLabel }: Props) {
  return (
    <>
      <button
        className="shrink-0 text-text-muted hover:text-status-failed self-end pb-0.5"
        onClick={onInterrupt}
        title="인터럽트 (Ctrl-C) — 진행 중 응답 중단"
        aria-label="인터럽트"
      >
        <Icon name="stop" size={15} />
      </button>
      <button
        className={`shrink-0 self-end pb-0.5 ${
          canSend ? "text-primary-bright" : "text-text-muted"
        }`}
        disabled={!canSend}
        onClick={onSend}
        aria-label={sendLabel ?? "전송"}
        title={`${sendLabel ?? "전송"} (Enter)`}
      >
        {sendLabel ? <span className="text-xs whitespace-nowrap">{sendLabel}</span> : <Icon name="send" size={18} />}
      </button>
    </>
  );
}
