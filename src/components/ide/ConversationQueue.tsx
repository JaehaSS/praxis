import type { ConversationQueueSnapshot } from "../../lib/conversation-queue";

interface Props {
  queue: ConversationQueueSnapshot;
  connected: boolean;
  onRemove: (id: string) => void;
  onPause: () => void;
  onResume: () => void;
}

export function ConversationQueue({ queue, connected, onRemove, onPause, onResume }: Props) {
  if (!queue.items.length) return null;
  return (
    <section aria-label="요청 대기열" className="mb-2 rounded border border-border bg-surface p-2 text-xs text-text-secondary">
      <div className="flex items-center justify-between gap-2">
        <span role="status">{queue.paused ? "일시정지" : "응답 후 자동 전송"} · {queue.items.length}개</span>
        <button type="button" className="rounded px-2 py-1 hover:bg-raised disabled:opacity-50"
          disabled={queue.paused && !connected} onClick={queue.paused ? onResume : onPause}>
          {queue.paused ? "계속 보내기" : "일시정지"}
        </button>
      </div>
      {queue.reason && <p role="alert" className="my-1 whitespace-pre-wrap text-status-awaiting">{queue.reason}</p>}
      <ol className="max-h-40 overflow-auto">
        {queue.items.map((item, index) => (
          <li key={item.id} className="flex items-start gap-2 border-t border-border py-1.5">
            <span className="pt-0.5">{index + 1}.</span>
            <details className="min-w-0 flex-1">
              <summary className="cursor-pointer truncate" title={item.message}>{item.message.split("\n")[0]}</summary>
              <p className="my-1 whitespace-pre-wrap break-words">{item.message}</p>
            </details>
            {item.images.length > 0 && <span className="shrink-0">이미지 {item.images.length}개</span>}
            {item.sending ? <span className="shrink-0">전송 중…</span>
              : item.uncertain ? <span className="shrink-0">접수 확인 필요</span>
              : <button type="button" aria-label={`대기 요청 ${index + 1} 삭제`} onClick={() => onRemove(item.id)}
                  className="shrink-0 rounded px-1 hover:text-status-failed">삭제</button>}
          </li>
        ))}
      </ol>
      <p className="mt-1 text-text-muted">앱이 열려 있는 동안 순서대로 보냅니다. 앱을 종료하면 대기열이 사라집니다.</p>
    </section>
  );
}
