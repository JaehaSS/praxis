import { useEffect, useRef, useState } from "react";
import type { Task } from "../../lib/ipc";
import { isTypingTarget } from "../../lib/typing-target";

/** 번호를 붙일 수 있는 최대 개수 — ⌘1‥⌘9. 넘치는 작업은 목록에서 눌러 연다. */
export const SHORTCUT_LIMIT = 9;

/**
 * ⌘를 이만큼 붙잡고 있어야 번호가 뜬다. ⌘K·⌘B처럼 스쳐 지나가는 조합도 ⌘ keydown은 똑같이
 * 보내므로, 지연이 없으면 단축키를 누를 때마다 사이드바가 번쩍인다.
 */
const HOLD_DELAY_MS = 300;

export interface SessionTaskShortcutOptions {
  /** 화면에 보이는 순서로 늘어놓은 작업 — 앞에서부터 1번. 접힌 프로젝트의 작업은 빠진다.
   *  id가 아니라 작업 자체를 든다: 호스트가 다른 두 작업이 같은 id를 가질 수 있다. */
  orderedTasks: Task[];
  /** Delete 대상 — 지금 열려 있고 목록에도 보이는 작업. 없으면 Delete는 무시된다. */
  deleteTarget: Task | null;
  onOpenTask: (task: Task) => void;
  onDeleteTask: (task: Task) => void;
}

/**
 * 사이드바 세션 목록의 키보드 조작 — ⌘ 홀드로 번호 표시(반환값), ⌘1‥⌘9로 이동, Delete로 삭제.
 * Delete는 삭제를 예약만 한다 — 중단도 워크트리 정리도 유예가 끝나야 일어나므로, 확인 대화상자로
 * 손을 멈추게 하는 대신 ⌘Z로 되돌릴 수 있게 한다(useDeferredTaskRemoval).
 */
export function useSessionTaskShortcuts(options: SessionTaskShortcutOptions): boolean {
  const [holding, setHolding] = useState(false);
  // 목록·선택은 매 렌더 바뀌지만 리스너는 한 번만 건다 — 홀드 중 재등록되면 타이머가 끊긴다.
  const latest = useRef(options);
  latest.current = options;

  useEffect(() => {
    let timer: number | null = null;
    const release = (): void => {
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
      setHolding(false);
    };
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Meta" && event.key !== "Control") return;
      if (timer !== null) return; // 홀드 중 반복 keydown — 타이머를 다시 감지 않는다
      timer = window.setTimeout(() => setHolding(true), HOLD_DELAY_MS);
    };
    const onKeyUp = (event: KeyboardEvent): void => {
      if (event.key === "Meta" || event.key === "Control") release();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    // ⌘Tab으로 앱을 떠나면 keyup이 오지 않는다 — 번호가 화면에 남는 것을 막는다.
    window.addEventListener("blur", release);
    document.addEventListener("visibilitychange", release);
    return () => {
      release();
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", release);
      document.removeEventListener("visibilitychange", release);
    };
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.defaultPrevented) return;
      const { orderedTasks, deleteTarget, onOpenTask, onDeleteTask } = latest.current;

      if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey) {
        // e.code로 본다 — 숫자열 배치가 다른 레이아웃에서도 물리 키 위치가 화면의 번호와 맞는다.
        const digit = /^Digit([1-9])$/.exec(event.code)?.[1];
        if (digit === undefined) return;
        const target = orderedTasks[Number(digit) - 1];
        if (target === undefined) return; // 비어 있는 번호 — 아무 일도 없는 편이 낫다
        event.preventDefault();
        setHolding(false); // 이동했으니 번호는 즉시 걷는다
        onOpenTask(target);
        return;
      }

      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key !== "Delete" && event.key !== "Backspace") return;
      if (isTypingTarget()) return;
      if (!deleteTarget) return;
      // 완료 처리 중인 작업은 정리 자체가 막혀 있다(task-removal). 확인을 받아 놓고 실패시키느니
      // 키를 삼킨다 — 마우스 경로에서는 그대로 이유가 표시된다.
      if (deleteTarget.state === "Finalizing") return;
      event.preventDefault();
      onDeleteTask(deleteTarget);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return holding;
}
