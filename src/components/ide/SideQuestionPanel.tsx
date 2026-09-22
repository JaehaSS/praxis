import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import {
  contextSourceHash,
  createQuestionReference,
  type QuestionReference,
  type SideQuestionApi,
  type SideQuestionContext,
  type SideQuestionInput,
  type SideQuestionSnapshot,
  type SideQuestionTurn,
} from "../../lib/side-question";
import { Markdown } from "./Markdown";
import { Icon } from "./icons";

export interface SideQuestionPanelProps {
  /** Host + task coordinate, never a bare numeric task id. */
  sessionKey: string;
  api: SideQuestionApi;
  /** Hidden tabs do no fetching or transcript rendering. */
  active: boolean;
  onAttach: (reference: QuestionReference) => void;
  onBack: () => void;
  initialContext?: { id: string; context: SideQuestionContext } | null;
  files?: string[];
  readFile?: (path: string) => Promise<string>;
}

type PendingDelivery = { input: SideQuestionInput; revision: number };
type SideDraft = {
  question: string;
  contexts: SideQuestionContext[];
  revision: number;
  pending: PendingDelivery | null;
  initialIds: Set<string>;
};

const draftStore = new Map<string, SideDraft>();

function emptyDraft(): SideDraft {
  return { question: "", contexts: [], revision: 0, pending: null, initialIds: new Set() };
}

function storedDraft(sessionKey: string): SideDraft {
  return draftStore.get(sessionKey) ?? emptyDraft();
}

function saveDraft(sessionKey: string, draft: SideDraft): void {
  draftStore.delete(sessionKey);
  if (draft.question !== "" || draft.contexts.length > 0 || draft.pending != null || draft.initialIds.size > 0) {
    draftStore.set(sessionKey, draft);
  }
}

function copyContext(context: SideQuestionContext): SideQuestionContext {
  return {
    label: context.label,
    text: context.text,
    path: context.path ?? null,
    ...(context.source_hash == null ? {} : { source_hash: context.source_hash }),
  };
}

function isBusy(turn: SideQuestionTurn): boolean {
  return turn.state === "queued" || turn.state === "running" || turn.state === "stopping";
}

function statusLabel(turn: SideQuestionTurn): string {
  switch (turn.state) {
    case "queued": return "대기 중";
    case "running": return "답변 중";
    case "stopping": return "질문을 중단하고 있습니다";
    case "completed": return "완료";
    case "failed": return "실패";
    case "cancelled": return "취소됨";
    case "interrupted": return "중단됨";
  }
}

function statusClass(turn: SideQuestionTurn): string {
  if (turn.state === "completed") return "text-status-done";
  if (turn.state === "failed" || turn.state === "cancelled" || turn.state === "interrupted") return "text-status-failed";
  if (turn.state === "queued") return "text-status-awaiting";
  return "text-status-running";
}

function requestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return `side-question-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function selectedAnswerText(fallback: string, answerElement: HTMLElement | null): string {
  const selection = typeof window === "undefined" ? null : window.getSelection();
  const selected = selection?.toString().trim() ?? "";
  if (
    selected !== "" && answerElement != null && selection?.anchorNode != null && selection.focusNode != null &&
    answerElement.contains(selection.anchorNode) && answerElement.contains(selection.focusNode)
  ) return selected;
  return fallback;
}

function contextKey(context: SideQuestionContext): string {
  return `${context.path ?? ""}\u0000${context.label}\u0000${context.text}\u0000${context.source_hash ?? ""}`;
}

type ContextSourceStatus = "changed" | "unavailable";

function sourceStatusLabel(context: SideQuestionContext, status: ContextSourceStatus): string {
  if (status === "unavailable") return "파일 상태를 확인할 수 없음";
  return context.source_hash == null ? "선택 부분 이후 변경됨" : "파일 이후 변경됨";
}

const RESET_CONFIRMATION =
  "이 따로 질문의 기록과 질문 초안을 비웁니다. 이미 메인 입력에 첨부한 참고자료는 남습니다. 계속할까요?";

/**
 * Independent, text-only question thread.  It accepts explicit material only
 * and can add a selected answer to the main draft, but never sends that draft.
 */
export function SideQuestionPanel({
  sessionKey,
  api,
  active,
  onAttach,
  onBack,
  initialContext = null,
  files = [],
  readFile,
}: SideQuestionPanelProps) {
  const [draft, setDraftState] = useState<SideDraft>(() => storedDraft(sessionKey));
  const [snapshot, setSnapshot] = useState<SideQuestionSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [sending, setSending] = useState(false);
  const [filePath, setFilePath] = useState("");
  const [textLabel, setTextLabel] = useState("선택한 텍스트");
  const [textContext, setTextContext] = useState("");
  const [editing, setEditing] = useState<{ turn: SideQuestionTurn; text: string } | null>(null);
  const [sourceStatuses, setSourceStatuses] = useState<Map<string, ContextSourceStatus>>(() => new Map());
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  const editingRef = useRef<HTMLTextAreaElement | null>(null);
  const answerRefs = useRef(new Map<number, HTMLDivElement>());
  const apiRef = useRef(api);
  const readFileRef = useRef(readFile);
  const sessionRef = useRef(sessionKey);
  const activeRef = useRef(active);
  const draftRef = useRef(draft);
  const checkingRef = useRef(false);
  const sendingRef = useRef(false);
  const mountedRef = useRef(true);

  apiRef.current = api;
  readFileRef.current = readFile;
  sessionRef.current = sessionKey;
  activeRef.current = active;
  draftRef.current = draft;

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  const replaceDraftForSession = useCallback((owner: string, next: SideDraft) => {
    saveDraft(owner, next);
    if (!mountedRef.current || sessionRef.current !== owner) return;
    draftRef.current = next;
    setDraftState(next);
  }, []);

  const replaceDraft = useCallback((next: SideDraft) => {
    replaceDraftForSession(sessionRef.current, next);
  }, [replaceDraftForSession]);

  const updateDraft = useCallback((change: (current: SideDraft) => SideDraft) => {
    const next = change(draftRef.current);
    replaceDraft(next);
  }, [replaceDraft]);

  // Restoring in layout effect avoids one paint of the previous task's text.
  useLayoutEffect(() => {
    const next = storedDraft(sessionKey);
    draftRef.current = next;
    setDraftState(next);
    setSnapshot(null);
    setError(null);
    checkingRef.current = false;
    sendingRef.current = false;
    setChecking(false);
    setSending(false);
    setEditing(null);
    setSourceStatuses(new Map());
  }, [sessionKey]);

  useEffect(() => {
    if (!active || initialContext == null) return;
    const current = draftRef.current;
    if (current.initialIds.has(initialContext.id)) return;
    const context = copyContext(initialContext.context);
    const next: SideDraft = {
      ...current,
      contexts: [...current.contexts, context],
      revision: current.revision + 1,
      initialIds: new Set([...current.initialIds, initialContext.id]),
    };
    replaceDraftForSession(sessionKey, next);
  }, [active, initialContext, replaceDraftForSession, sessionKey]);

  const acceptSnapshot = useCallback((next: SideQuestionSnapshot, request?: PendingDelivery) => {
    if (!mountedRef.current || sessionRef.current !== sessionKey) return;
    setSnapshot(next);
    setError(null);
    const pending = request ?? draftRef.current.pending;
    if (pending == null) return;
    const observed = next.turns.some((turn) => turn.request_id === pending.input.request_id);
    // `send` resolving is an acceptance receipt; `read` needs to observe its ID.
    if (request != null || observed) {
      updateDraft((current) =>
        current.revision === pending.revision
          ? { ...current, question: "", contexts: [], revision: current.revision + 1, pending: null }
          : { ...current, pending: null },
      );
    }
  }, [sessionKey, updateDraft]);

  const refresh = useCallback(async () => {
    if (!activeRef.current || checkingRef.current) return;
    const expectedSession = sessionRef.current;
    checkingRef.current = true;
    setChecking(true);
    try {
      const next = await apiRef.current.read();
      if (mountedRef.current && sessionRef.current === expectedSession && activeRef.current) acceptSnapshot(next);
    } catch (cause) {
      if (mountedRef.current && sessionRef.current === expectedSession) {
        setError(`연결 상태를 확인하지 못했습니다: ${cause instanceof Error ? cause.message : String(cause)}`);
      }
    } finally {
      if (mountedRef.current && sessionRef.current === expectedSession) {
        checkingRef.current = false;
        setChecking(false);
      }
    }
  }, [acceptSnapshot]);

  useEffect(() => {
    if (!active) return;
    void refresh();
  }, [active, sessionKey, refresh]);

  useEffect(() => {
    if (!active || snapshot == null || !snapshot.turns.some(isBusy)) return;
    const timer = window.setInterval(() => void refresh(), 1_500);
    return () => window.clearInterval(timer);
  }, [active, refresh, snapshot]);

  useEffect(() => {
    if (!active || editing != null) return;
    const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => window.clearTimeout(timer);
  }, [active, editing, sessionKey]);

  useLayoutEffect(() => {
    if (editing != null) editingRef.current?.focus();
  }, [editing?.turn.id]);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const owner = sessionKey;
    const checkSources = () => {
      const sources = [
        ...draftRef.current.contexts,
        ...(snapshot?.turns.flatMap((turn) => turn.contexts) ?? []),
      ].filter((context) => context.path != null);
      if (sources.length === 0) {
        if (!cancelled && mountedRef.current && sessionRef.current === owner) setSourceStatuses(new Map());
        return;
      }
      const reader = readFileRef.current;
      if (reader == null) {
        if (!cancelled && mountedRef.current && sessionRef.current === owner) {
          setSourceStatuses(new Map(sources.map((context) => [contextKey(context), "unavailable"] as const)));
        }
        return;
      }
      void Promise.all(sources.map(async (context): Promise<readonly [string, ContextSourceStatus] | null> => {
        try {
          const current = await reader(context.path!);
          if (context.source_hash != null) {
            return contextSourceHash(current) === context.source_hash ? null : [contextKey(context), "changed"];
          }
          // Legacy contexts know only a selected substring, so we can honestly
          // detect its removal but cannot claim that the complete file is fresh.
          return current.includes(context.text) ? null : [contextKey(context), "changed"];
        } catch {
          return [contextKey(context), "unavailable"];
        }
      })).then((statuses) => {
        if (cancelled || !mountedRef.current || sessionRef.current !== owner) return;
        setSourceStatuses(new Map(statuses.filter((status): status is readonly [string, ContextSourceStatus] => status != null)));
      });
    };
    checkSources();
    window.addEventListener("focus", checkSources);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", checkSources);
    };
  }, [active, draft.contexts, sessionKey, snapshot?.turns]);

  if (!active) return null;

  const unresolved = draft.pending != null;
  const activeTurn = snapshot?.turns.find(isBusy) ?? null;
  const canSend = snapshot?.supported === true && activeTurn == null && !sending && !sendingRef.current && !checking && !unresolved && draft.question.trim() !== "";

  const deliver = async (pending: PendingDelivery, expectedSession: string) => {
    if (sendingRef.current || !mountedRef.current || sessionRef.current !== expectedSession) return;
    sendingRef.current = true;
    setSending(true);
    setError(null);
    try {
      const next = await apiRef.current.send(pending.input);
      if (mountedRef.current && sessionRef.current === expectedSession) acceptSnapshot(next, pending);
    } catch {
      // The server may have accepted a request whose response was lost. Keep this exact payload and ID.
      if (mountedRef.current && sessionRef.current === expectedSession) {
        setError("전송 결과를 확인할 수 없습니다. 상태를 확인하거나 같은 요청을 다시 시도하세요.");
      }
    } finally {
      if (mountedRef.current && sessionRef.current === expectedSession) {
        sendingRef.current = false;
        setSending(false);
      }
    }
  };

  const send = () => {
    if (!canSend || snapshot == null || draftRef.current.pending != null || sendingRef.current) return;
    const input: SideQuestionInput = {
      request_id: requestId(),
      generation: snapshot.generation,
      question: draft.question.trim(),
      contexts: draft.contexts.map(copyContext),
    };
    const pending: PendingDelivery = { input, revision: draft.revision };
    updateDraft((current) => ({ ...current, pending }));
    void deliver(pending, sessionKey);
  };

  const retryPending = () => {
    const pending = draftRef.current.pending;
    if (pending == null || sendingRef.current) return;
    void deliver(pending, sessionKey);
  };

  const cancel = async (turn: SideQuestionTurn) => {
    const expectedSession = sessionKey;
    try {
      setError(null);
      const next = await apiRef.current.cancel(turn.id);
      if (mountedRef.current && sessionRef.current === expectedSession) acceptSnapshot(next);
    } catch (cause) {
      if (mountedRef.current && sessionRef.current === expectedSession) setError(`질문을 중단하지 못했습니다: ${cause instanceof Error ? cause.message : String(cause)}`);
    }
  };

  const reset = async () => {
    if (snapshot == null || activeTurn != null) return;
    if (typeof window !== "undefined" && !window.confirm(RESET_CONFIRMATION)) return;
    const expectedSession = sessionKey;
    try {
      const next = await apiRef.current.reset(snapshot.generation);
      if (!mountedRef.current || sessionRef.current !== expectedSession) return;
      acceptSnapshot(next);
      replaceDraftForSession(expectedSession, { ...emptyDraft(), initialIds: new Set(draftRef.current.initialIds) });
      setEditing(null);
    } catch (cause) {
      if (mountedRef.current && sessionRef.current === expectedSession) setError(`새 질문을 시작하지 못했습니다: ${cause instanceof Error ? cause.message : String(cause)}`);
    }
  };

  const addTextContext = () => {
    if (textContext.trim() === "") return;
    updateDraft((current) => ({
      ...current,
      contexts: [...current.contexts, { label: textLabel.trim() || "선택한 텍스트", text: textContext, path: null }],
      revision: current.revision + 1,
    }));
    setTextContext("");
  };

  const addFile = async () => {
    const path = filePath;
    const reader = readFileRef.current;
    const expectedSession = sessionKey;
    if (path === "" || reader == null) return;
    try {
      const content = await reader(path);
      if (!mountedRef.current || sessionRef.current !== expectedSession) return;
      const current = draftRef.current;
      replaceDraftForSession(expectedSession, {
        ...current,
        contexts: [...current.contexts, {
          label: path.split("/").pop() ?? path,
          text: content,
          path,
          source_hash: contextSourceHash(content),
        }],
        revision: current.revision + 1,
      });
      setFilePath("");
    } catch (cause) {
      if (mountedRef.current && sessionRef.current === expectedSession) setError(`파일을 읽지 못했습니다: ${cause instanceof Error ? cause.message : String(cause)}`);
    }
  };

  const keyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  };

  return (
    <section className="flex min-h-0 flex-1 flex-col bg-surface text-text" aria-label="따로 질문">
      <header className="flex items-center gap-2 border-b border-border px-4 py-3">
        <Icon name="chat" size={16} />
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold">따로 질문 · 현재 세션</h2>
          <p className="truncate text-xs text-text-muted">{snapshot?.model ? `${snapshot.model} · ` : ""}첨부한 내용과 이 질의의 대화만 참고합니다.</p>
          <p className="text-xs text-text-muted">이전 대화는 완료된 최근 8턴까지 참고하며, 긴 질문·답변·자료는 일부가 생략될 수 있습니다.</p>
        </div>
        <button type="button" className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-raised hover:text-text" onClick={onBack}>메인 대화로</button>
        <button type="button" className="rounded border border-border px-2 py-1 text-xs text-text-secondary hover:border-primary disabled:opacity-50" disabled={activeTurn != null || snapshot == null} onClick={() => void reset()}>새 질문으로 시작</button>
      </header>

      {snapshot?.supported === false ? (
        <div className="m-4 rounded border border-border bg-raised/40 p-3 text-sm">
          <p>이 실행 환경은 따로 질문을 지원하지 않습니다.</p>
          {snapshot.reason && <p className="mt-1 text-xs text-text-secondary">{snapshot.reason}</p>}
          <button type="button" className="mt-2 rounded border border-border px-2 py-1 text-xs text-primary-bright hover:border-primary" onClick={onBack}>메인 대화로</button>
        </div>
      ) : (
        <>
          <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
            {snapshot?.turns.length === 0 && <p className="text-sm text-text-secondary">질문을 보내면 이곳에만 답변이 쌓입니다.</p>}
            {snapshot?.turns.map((turn) => (
              <article key={turn.id} className="mb-4 border-b border-border pb-4 last:border-b-0">
                <div className="flex items-start gap-2">
                  <Icon name="chat" size={14} />
                  <p className="min-w-0 flex-1 whitespace-pre-wrap break-words text-md">{turn.question}</p>
                  <span className={`shrink-0 text-xs ${statusClass(turn)}`} aria-live="polite">{statusLabel(turn)}</span>
                </div>
                {turn.contexts.length > 0 && (
                  <div className="mt-1 flex flex-wrap gap-x-1.5 gap-y-0.5 pl-6 text-xs text-text-muted">
                    <span>자료:</span>
                    {turn.contexts.map((context, index) => {
                      const status = sourceStatuses.get(contextKey(context));
                      return (
                        <span key={`${contextKey(context)}-${index}`} className="inline-flex items-center gap-1">
                          <span>{context.label}</span>
                          {status != null && <span className="text-status-awaiting">{sourceStatusLabel(context, status)}</span>}
                        </span>
                      );
                    })}
                  </div>
                )}
                {turn.answer !== "" && <div ref={(node) => { if (node) answerRefs.current.set(turn.id, node); else answerRefs.current.delete(turn.id); }} className="mt-2 rounded border border-border bg-bg px-3 py-2 text-md"><Markdown text={turn.answer} stable={!isBusy(turn)} /></div>}
                {turn.error && <p className="mt-1 text-xs text-status-failed">{turn.error}</p>}
                <div className="mt-2 flex justify-end gap-1.5">
                  {isBusy(turn) ? (
                    <button type="button" className="rounded border border-border px-2 py-1 text-xs text-text-secondary hover:border-primary disabled:opacity-50" disabled={turn.state === "stopping"} onClick={() => void cancel(turn)}>{turn.state === "queued" ? "대기 취소" : "질문 중단"}</button>
                  ) : turn.answer !== "" ? (
                    <button type="button" className="rounded border border-primary px-2 py-1 text-xs text-primary-bright hover:bg-primary/10" onMouseDown={(event) => event.preventDefault()} onClick={() => setEditing({ turn, text: selectedAnswerText(turn.answer, answerRefs.current.get(turn.id) ?? null) })}>참고자료로 선택</button>
                  ) : null}
                </div>
                {editing?.turn.id === turn.id && (
                  <div className="mt-2 rounded border border-border bg-raised/40 p-2">
                    <p className="mb-1 text-xs text-text-secondary">선택 내용을 확인하고 편집하세요</p>
                    <textarea ref={editingRef} className="min-h-24 w-full resize-y rounded border border-border bg-bg px-2 py-1 text-sm outline-none focus:border-primary" aria-label="메인에 첨부할 답변" value={editing.text} onChange={(event) => setEditing({ ...editing, text: event.target.value })} onKeyDown={(event) => { if (!event.nativeEvent.isComposing && event.keyCode !== 229 && event.key === "Escape") { event.preventDefault(); setEditing(null); } }} />
                    <div className="mt-1.5 flex justify-end gap-1.5">
                      <button type="button" className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-raised" onClick={() => setEditing(null)}>취소</button>
                      <button type="button" className="rounded border border-primary px-2 py-1 text-xs text-primary-bright disabled:opacity-50" disabled={editing.text.trim() === ""} onClick={() => { onAttach(createQuestionReference(sessionKey, turn, editing.text)); setEditing(null); }}>메인 입력창에 첨부</button>
                    </div>
                  </div>
                )}
              </article>
            ))}
          </div>

          <div className="border-t border-border p-4">
            <div className="mb-2 flex flex-wrap gap-1.5" role="list" aria-label="질의 자료">
              {draft.contexts.map((context, index) => (
                <span key={`${context.label}-${index}`} role="listitem" className="flex max-w-full items-center gap-1 rounded border border-border px-2 py-0.5 text-xs text-text-secondary" title={context.text}>
                  <Icon name={context.path ? "fileText" : "code"} size={12} />
                  <span className="max-w-40 truncate">{context.label}</span>
                  {sourceStatuses.has(contextKey(context)) && <span className="shrink-0 text-status-awaiting">{sourceStatusLabel(context, sourceStatuses.get(contextKey(context))!)}</span>}
                  <button type="button" className="text-text-muted hover:text-status-failed" onClick={() => updateDraft((current) => ({ ...current, contexts: current.contexts.filter((_, itemIndex) => itemIndex !== index), revision: current.revision + 1 }))} aria-label={`${context.label} 자료 제거`}><Icon name="x" size={12} /></button>
                </span>
              ))}
            </div>
            {(files.length > 0 && readFile != null) && <div className="mb-2 flex gap-1.5"><label className="sr-only" htmlFor="side-question-file">파일 자료 추가</label><select id="side-question-file" className="min-w-0 flex-1 rounded border border-border bg-bg px-2 py-1 text-xs text-text" value={filePath} onChange={(event) => setFilePath(event.target.value)}><option value="">파일 자료 추가…</option>{files.map((path) => <option key={path} value={path}>{path}</option>)}</select><button type="button" className="rounded border border-border px-2 py-1 text-xs text-text-secondary hover:border-primary disabled:opacity-50" disabled={filePath === ""} onClick={() => void addFile()}>추가</button></div>}
            <details className="mb-2">
              <summary className="cursor-pointer text-xs text-text-secondary">텍스트 자료 추가</summary>
              <div className="mt-1 flex gap-1.5"><input className="w-28 rounded border border-border bg-bg px-2 py-1 text-xs text-text outline-none focus:border-primary" aria-label="텍스트 자료 이름" value={textLabel} onChange={(event) => setTextLabel(event.target.value)} /><textarea className="min-h-12 min-w-0 flex-1 resize-y rounded border border-border bg-bg px-2 py-1 text-xs text-text outline-none focus:border-primary" aria-label="텍스트 자료 내용" value={textContext} onChange={(event) => setTextContext(event.target.value)} /><button type="button" className="rounded border border-border px-2 py-1 text-xs text-text-secondary hover:border-primary disabled:opacity-50" disabled={textContext.trim() === ""} onClick={addTextContext}>추가</button></div>
            </details>
            <label className="sr-only" htmlFor="side-question-input">따로 질문</label>
            <textarea id="side-question-input" ref={inputRef} className="min-h-20 w-full resize-y rounded border border-border bg-bg px-3 py-2 text-md text-text outline-none placeholder:text-text-muted focus:border-primary" value={draft.question} placeholder="후속 질문을 입력하세요…" onChange={(event) => updateDraft((current) => ({ ...current, question: event.target.value, revision: current.revision + 1 }))} onKeyDown={keyDown} />
            <div className="mt-2 flex items-center justify-between gap-2"><div className="min-w-0 text-xs text-text-muted">{unresolved ? "연결 후 전송 상태를 확인합니다." : error ?? (activeTurn?.state === "queued" ? "동시 실행 한도에 여유가 생기면 질문합니다." : "Enter로 질문 보내기 · Shift+Enter 줄바꿈")}</div><div className="flex shrink-0 gap-1.5"><button type="button" className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-raised disabled:opacity-50" disabled={checking} onClick={() => void refresh()}>상태 확인</button>{unresolved && <button type="button" className="rounded border border-border px-2 py-1 text-xs text-text-secondary hover:border-primary disabled:opacity-50" disabled={sending} onClick={retryPending}>같은 요청 다시 시도</button>}<button type="button" className="rounded border border-primary bg-primary px-2 py-1 text-xs text-bg hover:bg-primary-hover disabled:border-border disabled:bg-raised disabled:text-text-muted" disabled={!canSend} onClick={send}>질문 보내기</button></div></div>
          </div>
        </>
      )}
    </section>
  );
}
