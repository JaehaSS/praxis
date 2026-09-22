import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { adoptTheme } from "../../lib/themes";
import { PreviewWorkbenchStrip } from "./PreviewWorkbenchStrip";
import {
  PREVIEW_TOOLBAR_EVENT,
  matchesToolbarIdentity,
  relayToolbarMessage,
  toolbarHeight,
  type ToolbarAction,
  type ToolbarIdentity,
  type ToolbarIntent,
  type ToolbarMessage,
  type ToolbarReady,
  type ToolbarState,
} from "../../lib/preview-workbench/window-events";
import type { PreviewWorkbenchState } from "../../lib/preview-workbench/types";

const waitingState: PreviewWorkbenchState = {
  key: "", taskId: 0, appEpoch: "", busy: "unknown", url: null, convoActive: false, takenOver: false,
  supported: false, unsupportedReason: "프리뷰 도구 모음 연결을 확인 중입니다.", draft: "", displayUrl: null,
  pending: null, inFlight: null, error: null, lastAction: null, revision: 0,
};
const TIMEOUT_MS = 3_000;

export function PreviewToolbarWindow() {
  const [state, setState] = useState(waitingState);
  const [identity, setIdentity] = useState<ToolbarIdentity | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const identityRef = useRef<ToolbarIdentity | null>(null);
  const revisionRef = useRef(-1);
  const sequenceRef = useRef(0);
  const instanceIdRef = useRef(globalThis.crypto.randomUUID());
  const draftRef = useRef<{ correlationId: string; text: string; submitted: boolean } | null>(null);
  const retryRef = useRef<ToolbarIntent | null>(null);
  const pendingRef = useRef(new Map<string, ToolbarIntent>());
  const timersRef = useRef(new Map<string, number>());
  const nextCorrelation = useCallback(() => `toolbar:${instanceIdRef.current}:${++sequenceRef.current}`, []);
  const clearPending = useCallback((correlationId: string) => {
    const timer = timersRef.current.get(correlationId);
    if (timer != null) globalThis.clearTimeout(timer);
    timersRef.current.delete(correlationId);
    pendingRef.current.delete(correlationId);
  }, []);
  const relay = useCallback(async (message: ToolbarIntent | ToolbarReady, awaiting = false) => {
    if (awaiting && message.kind === "intent") {
      const existing = timersRef.current.get(message.correlationId);
      if (existing != null) globalThis.clearTimeout(existing);
      pendingRef.current.set(message.correlationId, message);
      timersRef.current.set(message.correlationId, globalThis.setTimeout(() => {
        if (!pendingRef.current.has(message.correlationId)) return;
        timersRef.current.delete(message.correlationId);
        retryRef.current = message;
        setConnectionError("프리뷰 도구 모음 응답 시간이 초과되었습니다.");
      }, TIMEOUT_MS));
    }
    try {
      await relayToolbarMessage(message);
    } catch (cause) {
      if (message.kind === "intent") clearPending(message.correlationId);
      if (message.kind === "intent") retryRef.current = message;
      setConnectionError(String(cause));
    }
  }, [clearPending]);

  useEffect(() => {
    const unlisten = listen<ToolbarMessage>(PREVIEW_TOOLBAR_EVENT, ({ payload }) => {
      if (payload.kind === "error") {
        if (!identityRef.current || matchesToolbarIdentity(payload, identityRef.current)) {
          clearPending(payload.correlationId);
          if (retryRef.current?.correlationId === payload.correlationId && payload.requestId)
            retryRef.current = { ...retryRef.current, requestId: payload.requestId };
          setConnectionError(payload.error);
        }
        return;
      }
      if (payload.kind === "ack") {
        if (!identityRef.current || !matchesToolbarIdentity(payload, identityRef.current)) return;
        const pending = pendingRef.current.get(payload.correlationId);
        if (payload.status !== "queued" && !payload.requestId) return;
        clearPending(payload.correlationId);
        if (pending && payload.status !== "queued") retryRef.current = { ...pending, requestId: payload.requestId };
        if (draftRef.current?.submitted && draftRef.current.correlationId === payload.correlationId) {
          draftRef.current = null;
          setDraft(null);
        }
        setConnectionError(null);
        return;
      }
      if (payload.kind !== "state") return;
      const message = payload as ToolbarState;
      const next = { appEpoch: message.appEpoch, taskId: message.taskId, toolbarLabel: message.toolbarLabel, windowGeneration: message.windowGeneration };
      if (identityRef.current && !matchesToolbarIdentity(message, identityRef.current)) return;
      if (message.revision <= revisionRef.current) return;
      identityRef.current = next;
      revisionRef.current = message.revision;
      setIdentity(next);
      setState(message.state);
      setConnectionError(null);
      clearPending(message.correlationId);
      if (message.state.receipt?.correlationId) clearPending(message.state.receipt.correlationId);
      if (draftRef.current?.submitted && message.state.receipt?.correlationId === draftRef.current.correlationId) {
        draftRef.current = null;
        setDraft(null);
      } else if (!draftRef.current?.submitted && draftRef.current?.text === message.state.draft) {
        draftRef.current = null;
        setDraft(null);
      }
      adoptTheme(message.theme);
    });
    return () => void unlisten.then((dispose) => dispose());
  }, [clearPending]);
  useEffect(() => () => timersRef.current.forEach((timer) => globalThis.clearTimeout(timer)), []);

  useEffect(() => {
    if (identity) return;
    const ready = () => void relay({ kind: "ready", appEpoch: "", taskId: 0, toolbarLabel: "", windowGeneration: 0, correlationId: nextCorrelation() });
    ready();
    const timer = globalThis.setInterval(ready, 500);
    return () => globalThis.clearInterval(timer);
  }, [identity, nextCorrelation, relay]);

  const intent = useCallback((action: ToolbarAction, patch: Partial<ToolbarIntent> = {}) => {
    const current = identityRef.current;
    if (!current) return null;
    const message: ToolbarIntent = { ...current, kind: "intent", action, correlationId: nextCorrelation(), ...patch };
    void relay(message, action === "ask" || action === "cancel" || action === "take_over" || action === "release");
    return message;
  }, [nextCorrelation, relay]);

  const display = { ...state, draft: draft ?? state.draft, ...(connectionError ? { error: connectionError } : {}) };
  const height = toolbarHeight(display);
  useEffect(() => {
    if (identity) intent("resize", { height });
  }, [height, identity, intent]);
  const retry = () => {
    if (retryRef.current) void relay(retryRef.current, true);
    else {
      identityRef.current = null;
      revisionRef.current = -1;
      setIdentity(null);
    }
  };
  return <PreviewWorkbenchStrip
    state={display}
    compact
    onDraftChange={(text) => {
      setDraft(text);
      const message = intent("draft", { text });
      if (message) draftRef.current = { correlationId: message.correlationId, text, submitted: false };
    }}
    onSubmit={(text) => {
      const message = intent("ask", { text });
      if (message && draftRef.current?.text === text) draftRef.current = { correlationId: message.correlationId, text, submitted: true };
    }}
    onCancelPending={() => intent("cancel", { targetCorrelationId: state.pending?.correlationId })}
    onTakeOver={() => intent("take_over")}
    onRelease={() => intent("release")}
    onRefresh={retry}
  />;
}
