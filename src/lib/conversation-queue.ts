import { ConversationSubmitter, type ConversationAdmission } from "./conversation-submit";
import { taskKey, type TaskRef } from "./transport";

export interface QueuedPrompt {
  id: string;
  message: string;
  images: string[];
  sending: boolean;
  uncertain: boolean;
}

export interface ConversationQueueSnapshot {
  items: QueuedPrompt[];
  paused: boolean;
  reason: string | null;
}

interface SessionQueue extends ConversationQueueSnapshot {
  ref: TaskRef;
  revision: number;
}

export type QueueReadiness = "ready" | "busy" | { blocked: string };
export interface ConversationQueueDriver {
  readiness(ref: TaskRef): Promise<QueueReadiness>;
  admission(ref: TaskRef): ConversationAdmission;
  onSending?(ref: TaskRef, prompt: QueuedPrompt): void;
  onAccepted?(ref: TaskRef, prompt: QueuedPrompt): void;
  onFailed?(ref: TaskRef, prompt: QueuedPrompt): void;
}

/** App-owned, in-memory FIFO. A selected view never owns delivery or the next draft. */
export class ConversationQueue {
  private sessions = new Map<string, SessionQueue>();
  private checking = new Set<string>();
  private disposed = false;

  constructor(private submitter: ConversationSubmitter, private changed: () => void) {}

  snapshot(key: string): ConversationQueueSnapshot {
    const session = this.sessions.get(key);
    return {
      items: session?.items.map((item) => ({ ...item, images: [...item.images] })) ?? [],
      paused: session?.paused ?? false,
      reason: session?.reason ?? null,
    };
  }

  refs(): TaskRef[] {
    return [...this.sessions.values()].filter((s) => s.items.length > 0).map((s) => ({ ...s.ref }));
  }

  has(key: string): boolean { return (this.sessions.get(key)?.items.length ?? 0) > 0; }

  enqueue(ref: TaskRef, message: string, images: string[]): boolean {
    if (this.disposed || !message.trim()) return false;
    const key = taskKey(ref);
    let session = this.sessions.get(key);
    if (!session) {
      session = { ref: { ...ref }, items: [], paused: false, reason: null, revision: 0 };
      this.sessions.set(key, session);
    }
    session.items.push({ id: crypto.randomUUID(), message, images: [...images], sending: false, uncertain: false });
    this.changed();
    return true;
  }

  remove(key: string, id: string): void {
    const session = this.sessions.get(key);
    const item = session?.items.find((candidate) => candidate.id === id);
    if (!session || !item || item.sending || item.uncertain) return;
    session.items = session.items.filter((candidate) => candidate.id !== id);
    session.revision++;
    if (!session.items.length) this.sessions.delete(key);
    this.changed();
  }

  pause(key: string, reason = "자동 전송을 일시정지했습니다."): void {
    const session = this.sessions.get(key);
    if (!session || !session.items.length) return;
    session.paused = true;
    session.reason = reason;
    session.revision++;
    this.changed();
  }

  resume(key: string): void {
    const session = this.sessions.get(key);
    if (!session) return;
    session.paused = false;
    session.reason = null;
    session.revision++;
    this.changed();
  }

  /** One fresh readiness check and at most one admission per session per sweep. */
  async flush(ref: TaskRef, driver: ConversationQueueDriver): Promise<void> {
    const key = taskKey(ref);
    const session = this.sessions.get(key);
    if (this.disposed || !session || session.paused || !session.items.length || this.checking.has(key)) return;
    this.checking.add(key);
    const item = session.items[0];
    const revision = session.revision;
    const current = () => !this.disposed && this.sessions.get(key) === session
      && !session.paused && session.revision === revision && session.items[0] === item;
    try {
      // Receipt lookup is read-only and remains valid after cancellation/task finalization.
      if (item.uncertain) {
        const outcome = await this.submitter.reconcile(key, driver.admission(ref));
        if (this.disposed) return;
        if (outcome === "accepted") {
          session.items.shift();
          if (!session.items.length) this.sessions.delete(key);
          driver.onAccepted?.(ref, item);
          return;
        }
        if (outcome === "failed") item.uncertain = false;
        if (!current()) return;
      }
      const readiness = await driver.readiness(ref);
      if (!current()) return;
      if (typeof readiness === "object") { this.pause(key, readiness.blocked); return; }
      if (readiness === "busy") return;
      // A direct request with uncertain admission must be resolved by its existing UI first.
      if (!item.uncertain && this.submitter.inspect(key)) {
        this.pause(key, "이전 요청의 접수 상태를 먼저 확인한 뒤 계속 보내주세요.");
        return;
      }
      item.sending = true;
      this.changed();
      driver.onSending?.(ref, item);
      await this.submitter.send(key, driver.admission(ref), item.message, item.images);
      // Pause during admission stops the tail, but cannot retract an already accepted request.
      session.items.shift();
      if (!session.items.length) this.sessions.delete(key);
      if (!this.disposed) driver.onAccepted?.(ref, item);
    } catch (error) {
      if (this.disposed) return;
      if (!item.sending && !current()) return;
      if (item.sending) item.uncertain = this.submitter.inspect(key) !== null;
      this.pause(key, String(error));
      if (item.sending) driver.onFailed?.(ref, item);
    } finally {
      item.sending = false;
      this.checking.delete(key);
      if (!this.disposed) this.changed();
    }
  }

  dispose(): void { this.disposed = true; }
  activate(): void { this.disposed = false; }
}
