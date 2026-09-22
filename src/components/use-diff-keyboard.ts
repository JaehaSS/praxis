import { useEffect, useRef } from "react";
import type { DiffMode } from "./DiffPresentation";

export interface DiffKeyHandlers {
  nextHunk: () => void;
  prevHunk: () => void;
  nextFile: () => void;
  prevFile: () => void;
  setMode: (mode: DiffMode) => void;
  toggleViewed: () => void;
}

/** 주석을 쓰는 중에는 단축키가 발동하면 안 된다 — "j"를 누르면 글자 대신 화면이 뛴다. */
export function shouldHandleDiffKey(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return true;
  if (target.isContentEditable) return false;
  return !["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/** 파일 목록에서 delta만큼 이동한 인덱스. 선택이 없으면(-1) 방향과 무관하게 첫 파일로 간다 —
 *  -1을 0으로 먼저 당기면 다음 파일이 1이 되어 첫 파일을 건너뛴다. */
export function nextFileIndex(current: number, length: number, delta: number): number {
  if (length === 0) return -1;
  if (current < 0) return 0;
  return Math.max(0, Math.min(length - 1, current + delta));
}

/** 헤더들의 뷰포트 상대 top에서 다음/이전 hunk를 고른다. 뷰포트 상단을 지난 마지막 헤더가
 *  "현재"이므로, 다음은 그 아래 첫 헤더(below), 이전은 현재보다 하나 앞(below - 2)이다. */
export function nextHunkIndex(headerTops: number[], containerTop: number, delta: number): number {
  if (headerTops.length === 0) return -1;
  let below = headerTops.findIndex((top) => top > containerTop + 1);
  if (below === -1) below = headerTops.length;
  return Math.max(0, Math.min(headerTops.length - 1, delta > 0 ? below : below - 2));
}

/** S-04 와이어프레임에 명세된 단축키. 수식키 조합은 통과시켜 앱 전역 단축키와 겹치지 않게 한다.
 *
 *  `active`가 거짓인 인스턴스는 듣지 않는다. diff 탭은 분할·배경으로 여럿 마운트될 수 있고,
 *  전부가 듣는다면 `J` 한 번에 화면이 여러 번 뛴다 — 소유자는 포커스 그룹의 활성 탭 하나다
 *  (설계 DR-9). 리스너는 window에 남는다: 포커스가 변경 목록에 있어도 소유자는 그 탭이다. */
export function useDiffKeyboard(handlers: DiffKeyHandlers, active = true): void {
  // handlers는 매 렌더 새 객체다. ref로 고정하지 않으면 리스너를 렌더마다 다시 건다.
  const ref = useRef(handlers);
  ref.current = handlers;
  const activeRef = useRef(active);
  activeRef.current = active;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!activeRef.current) return;
      // Shift도 제외한다. key.toLowerCase() 탓에 Shift+J가 그냥 j로 읽히기 때문이다.
      if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
      if (!shouldHandleDiffKey(event.target)) return;
      const current = ref.current;
      const action: Record<string, (() => void) | undefined> = {
        j: current.nextHunk,
        k: current.prevHunk,
        "]": current.nextFile,
        "[": current.prevFile,
        u: () => current.setMode("unified"),
        s: () => current.setMode("split"),
        v: current.toggleViewed,
      };
      const run = action[event.key.toLowerCase()];
      if (!run) return;
      event.preventDefault();
      run();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
