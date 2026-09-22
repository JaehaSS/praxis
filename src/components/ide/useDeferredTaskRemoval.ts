import { useCallback, useEffect, useRef, useState } from "react";
import type { Task } from "../../lib/ipc";
import { REMOVAL_GRACE_MS } from "../../lib/task-removal";
import { taskKey } from "../../lib/transport";
import { isTypingTarget } from "../../lib/typing-target";

export interface PendingRemoval {
  task: Task;
  /** 이 시각이 지나면 확정 — 배너의 카운트다운이 읽는 값. */
  deadline: number;
}

export interface DeferredTaskRemovalOptions {
  /** 유예가 끝난 뒤 실제로 지우는 일. 오류 표시는 이 콜백의 몫이며, 여기서 처음으로 중단·워크트리 정리가 일어난다. */
  commit: (task: Task) => void | Promise<void>;
  /** 예약 즉시 — 목록에서 감췄으니 열려 있던 화면도 접는다. */
  onSchedule?: (task: Task) => void;
  /** ⌘Z로 되살렸을 때 — 접었던 화면을 되돌린다. */
  onUndo?: (task: Task) => void;
  graceMs?: number;
}

export interface DeferredTaskRemoval {
  /** 예약된 순서대로. 화면 목록에서는 빠져 있고, 배너에만 남아 있다. */
  pending: PendingRemoval[];
  schedule: (task: Task) => void;
  undo: () => void;
  isPending: (task: Task) => boolean;
}

/**
 * 삭제를 유예 창 뒤로 미루고 ⌘Z로 되돌리는 상태 머신.
 *
 * 창이 열려 있는 동안에는 아무 IPC도 부르지 않는다 — 실행 중인 세션은 계속 돌고, 워크트리도
 * 그대로다. 그래서 되돌리기가 "복원"이 아니라 "예약 취소"로 끝난다. 언마운트(창 종료)는
 * 확정이 아니라 취소로 처리한다. 지우지 못한 세션은 다시 지울 수 있지만, 지워버린 세션은
 * 되찾을 수 없다.
 */
export function useDeferredTaskRemoval(
  options: DeferredTaskRemovalOptions,
): DeferredTaskRemoval {
  const graceMs = options.graceMs ?? REMOVAL_GRACE_MS;
  const [pending, setPending] = useState<PendingRemoval[]>([]);
  // 리스너는 한 번만 걸고 콜백은 매 렌더 갱신 — stale 클로저로 옛 목록을 지우는 것을 막는다.
  const latest = useRef(options);
  latest.current = options;
  const timers = useRef(new Map<string, number>());
  const committing = useRef(new Set<string>());
  const [, publishCommitting] = useState(0);
  const pendingRef = useRef<PendingRemoval[]>(pending);
  pendingRef.current = pending;

  const write = useCallback((next: PendingRemoval[]): void => {
    pendingRef.current = next;
    setPending(next);
  }, []);

  const clearTimer = useCallback((key: string): void => {
    const timer = timers.current.get(key);
    if (timer === undefined) return;
    window.clearTimeout(timer);
    timers.current.delete(key);
  }, []);

  const isPending = useCallback(
    (task: Task): boolean => {
      const key = taskKey(task);
      return committing.current.has(key) || pendingRef.current.some((entry) => taskKey(entry.task) === key);
    },
    [],
  );

  const setCommitting = useCallback((key: string, value: boolean): void => {
    if (value) committing.current.add(key);
    else committing.current.delete(key);
    publishCommitting((version) => version + 1);
  }, []);

  const schedule = useCallback(
    (task: Task): void => {
      const key = taskKey(task);
      if (isPending(task)) {
        // 이미 카운트다운 중 — 다시 예약하지도, 남은 시간을 늘리지도 않는다(두 번 눌러 시간을
        // 벌 수 있으면 유예가 아니다). 다만 화면은 접는다. 예약된 세션을 다시 열 수 있는
        // 경로(Quick Open·알림)가 있어, 여기서 조용히 돌아가면 버튼이 죽은 것처럼 보인다.
        latest.current.onSchedule?.(task);
        return;
      }
      const timer = window.setTimeout(() => {
        timers.current.delete(key);
        write(pendingRef.current.filter((entry) => taskKey(entry.task) !== key));
        setCommitting(key, true);
        void Promise.resolve()
          .then(() => latest.current.commit(task))
          .catch(() => undefined)
          .finally(() => setCommitting(key, false));
      }, graceMs);
      timers.current.set(key, timer);
      write([...pendingRef.current, { task, deadline: Date.now() + graceMs }]);
      latest.current.onSchedule?.(task);
    },
    [graceMs, isPending, setCommitting, write],
  );

  const undo = useCallback((): void => {
    // 여러 건을 연달아 지웠다면 가장 최근 것부터 — 편집기의 되돌리기와 같은 순서.
    const last = pendingRef.current[pendingRef.current.length - 1];
    if (!last) return;
    clearTimer(taskKey(last.task));
    write(pendingRef.current.slice(0, -1));
    latest.current.onUndo?.(last.task);
  }, [clearTimer, write]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.defaultPrevented) return;
      if (!(event.metaKey || event.ctrlKey) || event.shiftKey || event.altKey) return;
      if (event.key.toLowerCase() !== "z") return;
      // 에디터·터미널·컴포저의 ⌘Z는 그쪽 되돌리기다. 되돌릴 삭제가 없을 때도 손대지 않는다.
      if (isTypingTarget()) return;
      if (pendingRef.current.length === 0) return;
      event.preventDefault();
      undo();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [undo]);

  useEffect(() => {
    const registered = timers.current;
    return () => {
      for (const timer of registered.values()) window.clearTimeout(timer);
      registered.clear();
    };
  }, []);

  return { pending, schedule, undo, isPending };
}
