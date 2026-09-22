import { useCallback, useEffect, useReducer, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { previewRelease, previewTakeOver, previewWorkbenchPrepare, previewWorkbenchReceipt, previewWorkbenchSend, previewWorkbenchState } from "../lib/ipc";
import { buildPreviewRequestContext, isControllablePreviewUrl } from "../lib/preview-workbench/context";
import { PreviewWorkbenchStore, previewPending } from "../lib/preview-workbench/store";
import type { PreviewReceiptOutcome, PreviewSource, PreviewWorkbenchState } from "../lib/preview-workbench/types";
export interface PreviewWorkbenchTask { key: string; taskId: number; supported: boolean; unsupportedReason: string | null; terminal: boolean; }
interface IdleEvent { taskId: number; }
interface PreviewControlEvent { task_id: number; op: string; target: string | null; changed: boolean | null; url: string; }
const controlLabel = (event: PreviewControlEvent): string => {
  const action = event.target ? `${event.op} ${event.target}` : event.op;
  return event.changed == null ? action : `${action} → ${event.changed ? "changed" : "unchanged"}`;
}; interface Options { tasks: PreviewWorkbenchTask[]; onAccepted?: (taskId: number, message: string, running: boolean) => void; }
export interface PreviewWorkbenchHost {
  stateFor: (key: string, taskId: number) => PreviewWorkbenchState; setDraft: (key: string, taskId: number, draft: string) => void; submit: (key: string, taskId: number, message: string, correlationId?: string) => Promise<void>;
  cancelPending: (key: string, taskId: number, correlationId?: string) => void; refresh: (key: string, taskId: number) => Promise<void>; takeOver: (key: string, taskId: number) => Promise<void>; release: (key: string, taskId: number) => Promise<void>; receiptFor: (key: string, correlationId: string) => PreviewReceiptOutcome | null | undefined;
}
export function usePreviewWorkbenchHost({ tasks, onAccepted }: Options): PreviewWorkbenchHost {
  const storeRef = useRef(new PreviewWorkbenchStore());
  const tasksRef = useRef(tasks);
  const acceptedRef = useRef(new Set<string>());
  const sequenceRef = useRef(0);
  const flushesRef = useRef(new Set<string>());
  const refreshGenerationRef = useRef(new Map<string, number>());
  const latestSubmissionRef = useRef(new Map<string, string>());
  const onAcceptedRef = useRef(onAccepted);
  const [, rerender] = useReducer((count) => count + 1, 0);
  tasksRef.current = tasks;
  onAcceptedRef.current = onAccepted;
  const taskFor = useCallback((key: string) => tasksRef.current.find((task) => task.key === key), []);
  const touch = useCallback(() => rerender(), []);
  const active = useCallback((key: string) => taskFor(key)?.supported === true, [taskFor]);
  const accept = useCallback((requestId: string, taskId: number, message: string, running: boolean) => {
    if (acceptedRef.current.has(requestId)) return;
    acceptedRef.current.add(requestId); onAcceptedRef.current?.(taskId, message, running);
  }, []);
  const flush = useCallback(async (key: string, taskId: number) => {
    if (flushesRef.current.has(key)) return;
    flushesRef.current.add(key);
    try {
    if (!active(key)) return;
    const flight = storeRef.current.claim(key, taskId);
    if (!flight) return;
    touch();
    const state = storeRef.current.get(key, taskId);
    const currentFlight = () => active(key)
      && storeRef.current.get(key, taskId).appEpoch === state.appEpoch
      && storeRef.current.get(key, taskId).inFlight?.correlationId === flight.correlationId;
    const url = flight.url ?? state.url;
    if (!url || !isControllablePreviewUrl(url)) {
      storeRef.current.fail(key, taskId, flight.correlationId, "이 페이지는 제어 가능한 로컬 프리뷰가 아닙니다.");
      touch();
      return;
    }
    try {
      const message = buildPreviewRequestContext(flight.message, url);
      const prepared = flight.requestId ? null : await previewWorkbenchPrepare(taskId, flight.correlationId, message, url, flight.source);
      if (!currentFlight()) return;
      if (prepared && ["rejected", "retired", "invalidated"].includes(prepared.status)) {
        storeRef.current.terminal(key, taskId, flight.correlationId, "프리뷰 질문을 수락하지 않았습니다.");
        touch();
        return;
      }
      const requestId = flight.requestId ?? prepared!.requestId;
      storeRef.current.bindRequest(key, taskId, flight.correlationId, requestId);
      touch();
      const receipt = await previewWorkbenchSend(taskId, requestId, message, url, flight.source);
      if (!currentFlight()) return;
      if (receipt.accepted) accept(receipt.requestId, taskId, message, receipt.running);
      if (receipt.accepted || receipt.status === "finished") { storeRef.current.acknowledge(key, taskId, flight.correlationId, receipt.status === "finished" ? "finished" : "accepted", receipt.requestId); storeRef.current.resolve(key, taskId, flight.correlationId); }
      else if (receipt.status === "prepared") storeRef.current.uncertain(key, taskId, flight.correlationId, "작업이 끝난 뒤 같은 요청을 다시 확인합니다.");
      else storeRef.current.terminal(key, taskId, flight.correlationId, "프리뷰 질문을 수락하지 않았습니다.");
    } catch (cause) {
      if (!currentFlight()) return;
      const requestId = storeRef.current.get(key, taskId).inFlight?.requestId;
      if (requestId) {
        try {
          const receipt = await previewWorkbenchReceipt(taskId, requestId);
          if (!currentFlight()) return;
          if (receipt.accepted) {
            accept(requestId, taskId, buildPreviewRequestContext(flight.message, url), receipt.running);
            storeRef.current.acknowledge(key, taskId, flight.correlationId, "accepted", requestId);
            storeRef.current.resolve(key, taskId, flight.correlationId);
            touch();
            return;
          }
          if (receipt.status !== "prepared") {
            storeRef.current.terminal(key, taskId, flight.correlationId, "프리뷰 질문을 수락하지 않았습니다.");
            touch();
            return;
          }
        } catch {}
      }
      if (!currentFlight()) return;
      storeRef.current.uncertain(key, taskId, flight.correlationId, String(cause));
    }
    touch();
    } finally {
      flushesRef.current.delete(key);
    }
  }, [accept, active, touch]);
  const refresh = useCallback(async (key: string, taskId: number) => {
    const generation = (refreshGenerationRef.current.get(key) ?? 0) + 1;
    refreshGenerationRef.current.set(key, generation);
    const task = taskFor(key);
    if (!task) return;
    if (!task.supported) {
      storeRef.current.unsupported(key, taskId, task.unsupportedReason);
      touch();
      return;
    }
    try {
      const remote = await previewWorkbenchState(taskId);
      if (!active(key) || refreshGenerationRef.current.get(key) !== generation) return;
      storeRef.current.sync(key, remote);
      touch();
      await flush(key, taskId);
    } catch (cause) {
      if (!active(key) || refreshGenerationRef.current.get(key) !== generation) return;
      storeRef.current.failQuery(key, taskId, String(cause));
      touch();
    }
  }, [active, flush, taskFor, touch]);
  const submit = useCallback(async (key: string, taskId: number, message: string, correlationId?: string) => {
    if (!message.trim()) return;
    if (!active(key)) return;
    const beforeRefresh = storeRef.current.get(key, taskId);
    const correlation = correlationId ?? `${beforeRefresh.appEpoch || key}:${++sequenceRef.current}`;
    if (!storeRef.current.reserveCorrelation(key, correlation, message)) {
      if (storeRef.current.correlationMessage(key, correlation) !== message) {
        storeRef.current.conflict(key, taskId);
        touch();
        return;
      }
      const existing = storeRef.current.get(key, taskId);
      if (existing.inFlight?.correlationId !== correlation) return;
      if (!existing.inFlight.requestId) {
        await refresh(key, taskId);
        return;
      }
      const currentFlight = () => active(key)
        && storeRef.current.get(key, taskId).appEpoch === existing.appEpoch
        && storeRef.current.get(key, taskId).inFlight?.correlationId === correlation
        && storeRef.current.get(key, taskId).inFlight?.requestId === existing.inFlight?.requestId;
      try {
        const receipt = await previewWorkbenchReceipt(taskId, existing.inFlight.requestId);
        if (!currentFlight()) return;
        if (receipt.accepted) {
          const url = existing.inFlight.url ?? existing.url;
          accept(receipt.requestId, taskId, url ? buildPreviewRequestContext(existing.inFlight.message, url) : existing.inFlight.message, receipt.running);
          storeRef.current.acknowledge(key, taskId, correlation, receipt.status === "finished" ? "finished" : "accepted", receipt.requestId);
          storeRef.current.resolve(key, taskId, correlation);
          touch();
          return;
        }
        if (receipt.status !== "prepared") {
          storeRef.current.terminal(key, taskId, correlation, "프리뷰 질문을 수락하지 않았습니다.");
          touch();
          return;
        }
      } catch {}
      await refresh(key, taskId);
      return;
    }
    latestSubmissionRef.current.set(key, correlation);
    await refresh(key, taskId);
    if (!active(key) || latestSubmissionRef.current.get(key) !== correlation) return;
    const state = storeRef.current.get(key, taskId);
    const source: PreviewSource = state.busy === "idle" ? "manual" : "preview_queue";
    storeRef.current.queue(key, taskId, previewPending(message, correlation, source));
    if (source === "preview_queue") storeRef.current.acknowledge(key, taskId, correlation, "queued");
    if (source === "preview_queue") storeRef.current.clearDraftIfUnchanged(key, taskId, message);
    touch();
    if (source === "manual") {
      await flush(key, taskId);
      if (!storeRef.current.get(key, taskId).error) storeRef.current.clearDraftIfUnchanged(key, taskId, message);
    }
  }, [active, flush, refresh, touch]);
  const cancelPending = useCallback((key: string, taskId: number, correlationId?: string) => {
    if (!active(key)) return;
    storeRef.current.cancel(key, taskId, correlationId); touch();
  }, [active, touch]);
  const setDraft = useCallback((key: string, taskId: number, draft: string) => {
    if (!active(key)) return;
    storeRef.current.setDraft(key, taskId, draft); touch();
  }, [active, touch]);
  const takeOver = useCallback(async (key: string, taskId: number) => {
    await previewTakeOver(taskId);
    await refresh(key, taskId);
  }, [refresh]);
  const release = useCallback(async (key: string, taskId: number) => {
    await previewRelease(taskId);
    await refresh(key, taskId);
  }, [refresh]);
  useEffect(() => {
    const known = new Set(tasks.map((task) => task.key));
    for (const state of storeRef.current.all()) if (!known.has(state.key)) {
      storeRef.current.remove(state.key);
      latestSubmissionRef.current.delete(state.key);
    }
    for (const task of tasks) if (task.terminal) storeRef.current.dispose(task.key, task.taskId, task.unsupportedReason);
    else if (!task.supported) storeRef.current.unsupported(task.key, task.taskId, task.unsupportedReason);
    touch();
  }, [tasks, touch]);
  useEffect(() => {
    const unlisten = listen<IdleEvent>("preview-workbench://idle", (event) => {
      const task = tasksRef.current.find((candidate) => candidate.taskId === event.payload.taskId && candidate.supported);
      if (task) void refresh(task.key, task.taskId);
    });
    return () => void unlisten.then((dispose) => dispose());
  }, [refresh]);
  useEffect(() => {
    const unlisten = listen<PreviewControlEvent>("designmode://control", (event) => {
      const task = tasksRef.current.find((candidate) => candidate.taskId === event.payload.task_id && candidate.supported);
      if (!task) return;
      storeRef.current.display(task.key, task.taskId, controlLabel(event.payload), event.payload.url);
      touch();
      void refresh(task.key, task.taskId);
    });
    return () => void unlisten.then((dispose) => dispose());
  }, [refresh, touch]);
  useEffect(() => {
    const unlisten = listen<number>("designmode://closed", (event) => {
      const task = tasksRef.current.find((candidate) => candidate.taskId === event.payload && candidate.supported);
      if (!task) return;
      storeRef.current.closed(task.key, task.taskId);
      touch();
    });
    return () => void unlisten.then((dispose) => dispose());
  }, [touch]);
  return {
    stateFor: (key, taskId) => storeRef.current.get(key, taskId),
    setDraft,
    submit,
    cancelPending,
    refresh,
    takeOver,
    release,
    receiptFor: (key, correlationId) => storeRef.current.outcome(key, correlationId),
  };
}
