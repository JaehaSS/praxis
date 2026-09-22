import { useCallback, useEffect, useReducer, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { taskActivity, type ConvoEvent, type Task } from "../lib/ipc";
import { ConversationQueue, type QueuedPrompt, type QueueReadiness } from "../lib/conversation-queue";
import type { ConversationSubmitter } from "../lib/conversation-submit";
import { getTransport, hasHost, LOCAL_HOST, taskKey, type TaskRef, type PraxisTransport } from "../lib/transport";

interface Options {
  submitter: ConversationSubmitter;
  tasks: Task[];
  onSending: (ref: TaskRef, prompt: QueuedPrompt) => void;
  onAccepted: (ref: TaskRef, prompt: QueuedPrompt) => void;
  onFailed: (ref: TaskRef, prompt: QueuedPrompt) => void;
}

export function useConversationQueue(options: Options) {
  const latest = useRef(options);
  latest.current = options;
  const [, render] = useReducer((n: number) => n + 1, 0);
  const queueRef = useRef<ConversationQueue | null>(null);
  if (!queueRef.current) queueRef.current = new ConversationQueue(options.submitter, render);
  const queue = queueRef.current;

  const flush = useCallback(async () => {
    // Batch shared observations within each sweep; no queries when the queue is empty.
    const lists = new Map<string, Promise<Task[]>>();
    const transports = new Map<string, PraxisTransport>();
    const transportFor = (ref: TaskRef) => {
      const transport = getTransport(ref.host);
      const bound = transports.get(ref.host);
      if (bound && bound !== transport) throw new Error("연결이 변경되었습니다. 연결 후 계속 보내기를 눌러주세요.");
      transports.set(ref.host, transport);
      return transport;
    };
    let activity: ReturnType<typeof taskActivity> | undefined;
    const readiness = async (ref: TaskRef): Promise<QueueReadiness> => {
      if (!hasHost(ref.host)) return { blocked: "연결이 끊겼습니다. 연결 후 계속 보내기를 눌러주세요." };
      const transport = transportFor(ref);
      let list = lists.get(ref.host);
      if (!list) { list = transport.taskList(); lists.set(ref.host, list); }
      const task = (await list).find((task) => task.id === ref.id);
      if (!hasHost(ref.host) || getTransport(ref.host) !== transport)
        return { blocked: "연결이 변경되었습니다. 연결 후 계속 보내기를 눌러주세요." };
      if (!task || task.stale || task.mode !== "conversation")
        return { blocked: "대화 상태를 확인할 수 없습니다. 요청을 보존했습니다." };
      if (["Running", "Starting", "Queued", "Finalizing"].includes(task.state)) return "busy";
      if (task.state !== "AwaitingReview")
        return { blocked: "작업이 종료되었거나 전송할 수 없는 상태입니다. 대기 요청을 확인해주세요." };
      if (ref.host === LOCAL_HOST) {
        activity ??= taskActivity();
        // task://state arrives just before the process reservation is released.
        if ((await activity).some((turn) => turn.task_id === ref.id)) return "busy";
      }
      return "ready";
    };
    await Promise.all(queue.refs().map((ref) => queue.flush(ref, {
      readiness,
      admission: (target) => {
        const transport = transportFor(target);
        return {
          submit: (id, message, images) => transport.conversationSubmit(target.id, id, message, images),
          receipt: (id) => transport.conversationReceipt(target.id, id),
        };
      },
      onSending: (target, prompt) => latest.current.onSending(target, prompt),
      onAccepted: (target, prompt) => latest.current.onAccepted(target, prompt),
      onFailed: (target, prompt) => latest.current.onFailed(target, prompt),
    })));
  }, [queue]);

  useEffect(() => {
    queue.activate();
    const interval = window.setInterval(() => void flush(), 1_000);
    const unlisten = listen<ConvoEvent>("convo://event", ({ payload }) => {
      if (payload.kind === "result" && payload.is_error) {
        queue.pause(taskKey({ host: LOCAL_HOST, id: payload.id }), "응답이 중단되거나 오류로 끝났습니다. 요청을 확인한 뒤 계속 보내주세요.");
      }
    });
    return () => {
      queue.dispose();
      window.clearInterval(interval);
      void unlisten.then((stop) => stop());
    };
  }, [queue, flush]);

  // State changes also wake the queue, including sessions that are not selected.
  useEffect(() => { void flush(); }, [options.tasks, flush]);

  return { queue, flush };
}
