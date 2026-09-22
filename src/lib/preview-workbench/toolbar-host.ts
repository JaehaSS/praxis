import { getActiveTheme } from "../themes";
import type { PreviewWorkbenchHost } from "../../components/use-preview-workbench-host";
import { publishToolbarMessage, type ToolbarIntent, type ToolbarMessage } from "./window-events";
import { ToolbarSessions, type ToolbarSession } from "./toolbar-sessions";

type HostRef = { current: PreviewWorkbenchHost };

export class ToolbarHost {
  private readonly sessions = new ToolbarSessions();
  private publishing = false;
  private publishAgain = false;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly hostRef: HostRef) {}

  async handle(message: ToolbarMessage): Promise<void> {
    if (message.kind === "ready") {
      const session = this.sessions.register(message);
      await this.run(session, message.correlationId, () => this.hostRef.current.refresh(this.key(session), session.taskId));
      return;
    }
    if (message.kind !== "intent") return;
    const session = this.sessions.get(message);
    if (!session) return;
    const next = message.action === "resize" ? session : this.sessions.updateCorrelation(session, message.correlationId);
    await this.run(next, message.correlationId, () => this.intent(next, message), message.requestId);
  }

  async publish(): Promise<void> {
    this.cancelRetry();
    this.publishAgain = true;
    if (this.publishing) return;
    this.publishing = true;
    try {
      do {
        this.publishAgain = false;
        await this.publishSnapshots();
      } while (this.publishAgain);
    } finally {
      this.publishing = false;
    }
  }

  private async publishSnapshots(): Promise<void> {
    for (const session of this.sessions.all()) {
      const state = this.hostRef.current.stateFor(this.key(session), session.taskId);
      const theme = getActiveTheme();
      const revision = this.sessions.publication(session, state, theme);
      if (revision == null) continue;
      const correlationId = session.correlationId;
      try {
        await publishToolbarMessage({ ...session, kind: "state", correlationId, revision, state, theme });
        if (!this.sessions.isCurrent(session)) continue;
        await this.ack(session, state.receipt);
        this.sessions.commitPublication(session, state, theme, revision);
      } catch {
        if (this.sessions.isCurrent(session)) this.scheduleRetry();
      }
    }
  }

  clear(): void { this.cancelRetry(); this.publishAgain = false; this.sessions.clear(); }
  closeTask(taskId: number): void {
    for (const session of this.sessions.all()) if (session.taskId === taskId) this.sessions.close(session.toolbarLabel);
    if (this.sessions.all().length === 0) this.cancelRetry();
  }

  private cancelRetry(): void {
    if (this.retryTimer != null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
  }

  private scheduleRetry(): void {
    if (this.retryTimer != null) return;
    this.retryTimer = setTimeout(() => { this.retryTimer = null; void this.publish(); }, 500);
  }

  private async intent(session: ToolbarSession, message: ToolbarIntent): Promise<void> {
    const host = this.hostRef.current;
    const key = this.key(session);
    if (message.action === "ask" && message.text) {
      const state = host.stateFor(key, session.taskId);
      const boundRequestId = state.inFlight?.correlationId === message.correlationId
        ? state.inFlight.requestId
        : host.receiptFor(key, message.correlationId)?.requestId;
      if (message.requestId && boundRequestId !== message.requestId)
        throw new Error("요청 ID가 현재 프리뷰 영수증과 일치하지 않습니다.");
      await host.submit(key, session.taskId, message.text, message.correlationId);
      await this.ack(session, host.receiptFor(key, message.correlationId));
      return;
    }
    if (message.action === "cancel") host.cancelPending(key, session.taskId, message.targetCorrelationId);
    else if (message.action === "draft") host.setDraft(key, session.taskId, message.text ?? "");
    else if (message.action === "take_over") await host.takeOver(key, session.taskId);
    else if (message.action === "release") await host.release(key, session.taskId);
  }

  private async ack(session: ToolbarSession, outcome: ReturnType<PreviewWorkbenchHost["receiptFor"]>): Promise<void> {
    if (!outcome) return;
    if (outcome.status === "queued") await publishToolbarMessage({ ...session, kind: "ack", correlationId: outcome.correlationId, status: "queued" });
    else if (outcome.requestId) await publishToolbarMessage({ ...session, kind: "ack", correlationId: outcome.correlationId, status: outcome.status, requestId: outcome.requestId });
  }

  private async run(session: ToolbarSession, correlationId: string, action: () => Promise<void> | void, requestId?: string): Promise<void> {
    try { await action(); }
    catch (cause) { await publishToolbarMessage({ ...session, kind: "error", correlationId, ...(requestId ? { requestId } : {}), error: String(cause) }).catch(() => undefined); }
  }

  private key(session: ToolbarSession): string { return `local:${session.taskId}`; }
}
