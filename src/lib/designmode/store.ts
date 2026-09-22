import type { DesignCaptureRecord } from "./types";

/** 캡처 → Composer 칩 in-memory pub-sub. 같은 레포의 여러 세션이 섞이지 않도록 task id로 격리한다. */
type Listener = (records: DesignCaptureRecord[]) => void;

const byTask = new Map<number, DesignCaptureRecord[]>();
const listeners = new Map<number, Set<Listener>>();

function notify(taskId: number): void {
  const records = byTask.get(taskId) ?? [];
  for (const listener of listeners.get(taskId) ?? []) listener(records);
}

export function pushCapture(taskId: number, record: DesignCaptureRecord): void {
  byTask.set(taskId, [...(byTask.get(taskId) ?? []), record]);
  notify(taskId);
}

export function removeCapture(taskId: number, id: string): void {
  byTask.set(
    taskId,
    (byTask.get(taskId) ?? []).filter((record) => record.id !== id),
  );
  notify(taskId);
}

export function clearCaptures(taskId: number): void {
  byTask.set(taskId, []);
  notify(taskId);
}

export function getCaptures(taskId: number): DesignCaptureRecord[] {
  return byTask.get(taskId) ?? [];
}

/** 반환된 함수를 언마운트 시 호출해 구독을 해제한다. */
export function subscribeCaptures(taskId: number, listener: Listener): () => void {
  const set = listeners.get(taskId) ?? new Set();
  set.add(listener);
  listeners.set(taskId, set);
  return () => set.delete(listener);
}
