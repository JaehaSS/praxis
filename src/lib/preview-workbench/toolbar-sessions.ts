import type { ThemePayload } from "../theme-sync";
import type { PreviewWorkbenchState } from "./types";
import type { ToolbarEnvelope, ToolbarIdentity } from "./window-events";

export interface ToolbarSession extends ToolbarIdentity { correlationId: string; }

export class ToolbarSessions {
  private readonly sessions = new Map<string, ToolbarSession>();
  private readonly published = new Map<string, { sourceRevision: number; revision: number; theme: string }>();
  private readonly issuedRevision = new Map<string, number>();

  register(message: ToolbarEnvelope): ToolbarSession {
    for (const [label, session] of this.sessions) {
      if (session.taskId === message.taskId && label !== message.toolbarLabel) this.close(label);
    }
    const previous = this.sessions.get(message.toolbarLabel);
    if (previous && (previous.appEpoch !== message.appEpoch || previous.taskId !== message.taskId || previous.windowGeneration !== message.windowGeneration))
      this.published.delete(message.toolbarLabel);
    const session = { appEpoch: message.appEpoch, taskId: message.taskId, toolbarLabel: message.toolbarLabel, windowGeneration: message.windowGeneration, correlationId: message.correlationId };
    this.sessions.set(session.toolbarLabel, session);
    return session;
  }

  get(message: ToolbarEnvelope): ToolbarSession | undefined {
    const session = this.sessions.get(message.toolbarLabel);
    if (!session || session.appEpoch !== message.appEpoch || session.taskId !== message.taskId || session.windowGeneration !== message.windowGeneration) return undefined;
    return session;
  }

  updateCorrelation(session: ToolbarSession, correlationId: string): ToolbarSession {
    const next = { ...session, correlationId };
    this.sessions.set(next.toolbarLabel, next);
    return next;
  }

  all(): ToolbarSession[] { return [...this.sessions.values()]; }
  close(label: string): void { this.sessions.delete(label); this.published.delete(label); this.issuedRevision.delete(label); }
  clear(): void { this.sessions.clear(); this.published.clear(); this.issuedRevision.clear(); }
  isCurrent(session: ToolbarSession): boolean { return this.sessions.get(session.toolbarLabel) === session; }

  publication(session: ToolbarSession, state: PreviewWorkbenchState, theme: ThemePayload): number | null {
    if (this.sessions.get(session.toolbarLabel) !== session) return null;
    const previous = this.published.get(session.toolbarLabel);
    const serializedTheme = JSON.stringify(theme);
    if (previous?.sourceRevision === state.revision && previous.theme === serializedTheme) return null;
    const revision = Math.max(state.revision, (this.issuedRevision.get(session.toolbarLabel) ?? 0) + 1);
    this.issuedRevision.set(session.toolbarLabel, revision);
    return revision;
  }

  commitPublication(session: ToolbarSession, state: PreviewWorkbenchState, theme: ThemePayload, revision: number): void {
    if (this.isCurrent(session)) this.published.set(session.toolbarLabel, {
      sourceRevision: state.revision, revision, theme: JSON.stringify(theme),
    });
  }
}
