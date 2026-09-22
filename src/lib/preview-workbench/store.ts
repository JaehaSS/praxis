import type { PreviewFlight, PreviewPending, PreviewSource, PreviewWorkbenchRemoteState, PreviewWorkbenchState } from "./types";

const emptyState = (key: string, taskId: number): PreviewWorkbenchState => ({
  key,
  taskId,
  appEpoch: "",
  busy: "unknown",
  url: null,
  convoActive: false,
  takenOver: false,
  supported: false,
  unsupportedReason: null,
  draft: "",
  displayUrl: null,
  pending: null,
  inFlight: null,
  error: null,
  lastAction: null,
  revision: 0,
});

export class PreviewWorkbenchStore {
  private readonly states = new Map<string, PreviewWorkbenchState>();
  private readonly correlations = new Map<string, Map<string, string>>();
  private readonly outcomes = new Map<string, Map<string, PreviewWorkbenchState["receipt"]>>();
  private readonly terminalErrors = new Set<string>();

  get(key: string, taskId: number): PreviewWorkbenchState {
    const state = this.states.get(key) ?? emptyState(key, taskId);
    this.states.set(key, state);
    return state;
  }

  all(): PreviewWorkbenchState[] { return [...this.states.values()]; }

  setDraft(key: string, taskId: number, draft: string): PreviewWorkbenchState { return this.update(key, taskId, { draft }); }

  queue(key: string, taskId: number, pending: PreviewPending): PreviewWorkbenchState {
    const terminalError = this.terminalErrors.delete(key);
    return this.update(key, taskId, { pending, receipt: null, ...(terminalError ? { error: null } : {}) });
  }

  reserveCorrelation(key: string, correlationId: string, message: string): boolean {
    const correlations = this.correlations.get(key) ?? new Map<string, string>();
    if (correlations.has(correlationId)) return false;
    correlations.set(correlationId, message);
    this.correlations.set(key, correlations);
    return true;
  }

  correlationMessage(key: string, correlationId: string): string | undefined { return this.correlations.get(key)?.get(correlationId); }

  sync(key: string, remote: PreviewWorkbenchRemoteState): PreviewWorkbenchState {
    const previous = this.get(key, remote.taskId);
    const changedEpoch = previous.appEpoch !== "" && previous.appEpoch !== remote.appEpoch;
    if (changedEpoch) this.terminalErrors.delete(key);
    return this.update(key, remote.taskId, {
      ...remote,
      displayUrl: remote.url,
      ...(changedEpoch ? { pending: null, inFlight: null } : {}),
      error: changedEpoch ? "앱 연결이 변경되었습니다. 질문을 확인한 뒤 다시 보내세요." : this.terminalErrors.has(key) ? previous.error : null,
    });
  }

  failQuery(key: string, taskId: number, error: string): PreviewWorkbenchState { return this.update(key, taskId, { busy: "unknown", error }); }

  unsupported(key: string, taskId: number, reason: string | null): PreviewWorkbenchState {
    return this.update(key, taskId, {
      busy: "unknown",
      supported: false,
      unsupportedReason: reason,
      url: null,
      displayUrl: null,
      takenOver: false,
    });
  }

  claim(key: string, taskId: number): PreviewFlight | null {
    const state = this.get(key, taskId);
    if (state.busy !== "idle" || !state.supported || !state.url || !state.pending)
      return null;
    if (state.inFlight) return state.inFlight;
    const flight: PreviewFlight = { ...state.pending, url: state.url };
    this.update(key, taskId, { inFlight: flight });
    return flight;
  }

  bindRequest(key: string, taskId: number, correlationId: string, requestId: string): void {
    const state = this.get(key, taskId);
    if (state.inFlight?.correlationId !== correlationId) return;
    this.update(key, taskId, { inFlight: { ...state.inFlight, requestId } });
  }

  acknowledge(key: string, taskId: number, correlationId: string, status: "queued" | "accepted" | "finished", requestId?: string): void {
    const receipt = { correlationId, status, ...(requestId ? { requestId } : {}) };
    const outcomes = this.outcomes.get(key) ?? new Map();
    outcomes.set(correlationId, receipt);
    this.outcomes.set(key, outcomes);
    this.update(key, taskId, { receipt });
  }

  outcome(key: string, correlationId: string): PreviewWorkbenchState["receipt"] {
    return this.outcomes.get(key)?.get(correlationId) ?? null;
  }

  conflict(key: string, taskId: number): void { this.update(key, taskId, { error: "같은 요청 ID에는 처음 질문과 동일한 내용만 보낼 수 있습니다." }); }

  resolve(key: string, taskId: number, correlationId: string): void {
    const state = this.get(key, taskId);
    if (state.inFlight?.correlationId !== correlationId) return;
    const pending = state.pending?.correlationId === correlationId ? null : state.pending;
    this.terminalErrors.delete(key);
    this.update(key, taskId, { pending, inFlight: null, error: null });
  }

  fail(key: string, taskId: number, correlationId: string, error: string): void {
    const state = this.get(key, taskId);
    if (state.inFlight?.correlationId !== correlationId) return;
    this.update(key, taskId, { inFlight: null, error });
  }

  terminal(key: string, taskId: number, correlationId: string, error: string): void {
    const state = this.get(key, taskId);
    if (state.inFlight?.correlationId !== correlationId) return;
    const pending = state.pending?.correlationId === correlationId ? null : state.pending;
    this.terminalErrors.add(key);
    const draft = state.pending?.correlationId === correlationId && !state.draft ? state.pending.message : state.draft;
    this.update(key, taskId, { draft, pending, inFlight: null, error });
  }

  uncertain(key: string, taskId: number, correlationId: string, error: string): void {
    const state = this.get(key, taskId);
    if (state.inFlight?.correlationId !== correlationId) return;
    this.update(key, taskId, { error });
  }

  cancel(key: string, taskId: number, correlationId?: string): void {
    if (correlationId && this.get(key, taskId).pending?.correlationId !== correlationId) return;
    this.update(key, taskId, { pending: null, error: null });
  }

  display(key: string, taskId: number, lastAction: string, url: string | null): void { this.update(key, taskId, { lastAction, displayUrl: url }); }

  closed(key: string, taskId: number): void { this.update(key, taskId, { url: null, displayUrl: null, busy: "unknown" }); }

  dispose(key: string, taskId: number, reason: string | null): void {
    this.correlations.delete(key);
    this.outcomes.delete(key);
    this.terminalErrors.delete(key);
    this.update(key, taskId, {
      busy: "unknown", supported: false, unsupportedReason: reason, url: null, displayUrl: null,
      takenOver: false, draft: "", pending: null, inFlight: null, error: null, lastAction: null,
      receipt: null,
    });
  }

  clearDraftIfUnchanged(key: string, taskId: number, submitted: string): void { if (this.get(key, taskId).draft === submitted) this.update(key, taskId, { draft: "" }); }

  remove(key: string): void {
    this.states.delete(key);
    this.correlations.delete(key);
    this.outcomes.delete(key);
    this.terminalErrors.delete(key);
  }

  private update(
    key: string,
    taskId: number,
    patch: Partial<PreviewWorkbenchState>,
  ): PreviewWorkbenchState {
    const next = { ...this.get(key, taskId), ...patch };
    next.revision += 1;
    this.states.set(key, next);
    return next;
  }
}

export function previewPending(message: string, correlationId: string, source: PreviewSource): PreviewPending {
  return { message, correlationId, source };
}
