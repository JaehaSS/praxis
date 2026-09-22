import { useEffect, useRef, useState } from "react";
import {
  notificationAcknowledge,
  type InboxItem,
  type NotificationSnapshot,
} from "../../lib/notifications";

const kindLabel: Record<InboxItem["kind"], string> = {
  result: "결과 도착",
  question: "답변 필요",
  failure: "실패",
};

const projectName = (repo: string) => repo.split("/").filter(Boolean).pop() ?? repo;
const timeLabel = (ts: number) => new Date(ts * 1000).toLocaleString();

interface Props {
  snapshot: NotificationSnapshot | null;
  error: string | null;
  onRetry: () => void;
  onSnapshot: (snapshot: NotificationSnapshot) => void;
  onResult: (item: InboxItem) => Promise<boolean>;
  onChanges: (item: InboxItem) => Promise<void>;
}

export function NotificationInbox({ snapshot, error, onRetry, onSnapshot, onResult, onChanges }: Props) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionNotice, setActionNotice] = useState<string | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const close = () => {
    setOpen(false);
    trigger.current?.focus();
  };
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);
  const acknowledge = async (item: InboxItem) => {
    setBusy(`ack:${item.host}:${item.task_id}`);
    try {
      onSnapshot(await notificationAcknowledge(item.host, item.source_id, item.task_id, item.sequence));
      setActionError(null);
      setActionNotice(null);
    } catch (reason) {
      setActionError(String(reason));
    } finally {
      setBusy(null);
    }
  };
  const viewResult = async (item: InboxItem) => {
    setBusy(`result:${item.host}:${item.task_id}`);
    try {
      setActionError(null);
      setActionNotice(null);
      if (await onResult(item)) await acknowledge(item);
      else setActionNotice("결과를 연 뒤 확인을 눌러 읽음으로 표시하세요.");
    } catch (reason) {
      setActionError(String(reason));
    } finally {
      setBusy(null);
    }
  };
  const viewChanges = async (item: InboxItem) => {
    setBusy(`changes:${item.host}:${item.task_id}`);
    try {
      await onChanges(item);
      setActionError(null);
    } catch (reason) {
      setActionError(String(reason));
    } finally {
      setBusy(null);
    }
  };
  const count = snapshot?.items.length;
  return (
    <section className="shrink-0 border-t border-border bg-surface text-xs">
      <div className="flex h-8 items-center gap-2 px-3">
        <button ref={trigger} onClick={() => setOpen((value) => !value)} aria-expanded={open} className="text-text-secondary hover:text-text">
          {count == null ? "알림 불러오는 중" : count ? `미확인 ${count}건` : "미확인 없음"}
          {(error || snapshot?.delivery_error) && " · 알림 오류"}
        </button>
        {open && <button onClick={close} className="ml-auto text-text-muted hover:text-text">닫기</button>}
      </div>
      {open && (
        <div className="max-h-52 overflow-auto border-t border-border">
          {(error || actionError) && <Error text={actionError ?? error!} onRetry={() => { setActionError(null); onRetry(); }} />}
          {actionNotice && <p role="status" className="px-3 py-2 text-text-muted">{actionNotice}</p>}
          {snapshot?.delivery_error && <p className="px-3 py-2 text-status-failed">{snapshot.delivery_error}</p>}
          {snapshot?.sources.filter((source) => source.warning).map((source) => <p key={`${source.host}:${source.warning}`} className="px-3 py-2 text-text-muted">{source.warning}</p>)}
          {snapshot == null ? <p className="px-3 py-2 text-text-muted">알림 불러오는 중</p> : snapshot.items.length === 0 ? <p className="px-3 py-2 text-text-muted">미확인 없음</p> : snapshot.items.map((item) => (
            <div key={`${item.host}:${item.source_id}:${item.task_id}`} className="border-b border-border px-3 py-2 last:border-0 sm:flex sm:items-center sm:gap-3">
              <div className="min-w-0 flex-1">
                <div className="truncate text-text">{item.title}</div>
                <div className="text-text-muted">{projectName(item.repo)} · {item.host} · {kindLabel[item.kind]} · {timeLabel(item.ts)}</div>
              </div>
              <div className="mt-1 flex shrink-0 gap-2 sm:mt-0">
                <Action label="결과 보기" busy={busy !== null} onClick={() => void viewResult(item)} />
                <Action label="변경 보기" busy={busy !== null} onClick={() => void viewChanges(item)} />
                <Action label="확인" busy={busy !== null} onClick={() => void acknowledge(item)} />
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function Action({ label, busy, onClick }: { label: string; busy: boolean; onClick: () => void }) {
  return <button disabled={busy} onClick={onClick} className="text-text-secondary hover:text-text disabled:opacity-50">{label}</button>;
}

function Error({ text, onRetry }: { text: string; onRetry: () => void }) {
  return <div role="alert" className="flex items-center gap-2 px-3 py-2 text-status-failed"><span className="truncate">{text}</span><button onClick={onRetry} className="shrink-0 text-text-secondary hover:text-text">다시 시도</button></div>;
}
