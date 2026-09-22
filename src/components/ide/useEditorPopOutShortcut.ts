import { useEffect, useRef } from "react";

export interface EditorPopOutShortcutOptions {
  /** 이 화면에서 단축키를 들을지. 워크스페이스 밖에는 뺄 에디터가 없다. */
  active: () => boolean;
  /** 나가 있으면 그 창을 앞으로, 아니면 빼낸다 — 판단은 호출부의 몫이다. */
  onToggle: () => void;
}

/**
 * 에디터 팝아웃 토글 — ⌥⌘E.
 *
 * 코드 열을 거치지 않는 유일한 전역 진입로다(마우스 손잡이는 세션 헤더에만 있다).
 * `useSessionTaskShortcuts`와 같은 형태로 리스너를 한 번만 걸고 최신 콜백은 ref로 읽는다.
 */
export function useEditorPopOutShortcut(options: EditorPopOutShortcutOptions): void {
  const latest = useRef(options);
  latest.current = options;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.defaultPrevented) return;
      if (!latest.current.active()) return;
      // 눌러 두면 keydown이 반복된다 — 창 열기 IPC를 연타로 보내지 않는다.
      if (event.repeat) return;
      if (!(event.metaKey || event.ctrlKey) || !event.altKey || event.shiftKey) return;
      // e.code로 본다 — 레이아웃이 달라도 물리 키 위치가 같다.
      if (event.code !== "KeyE") return;
      event.preventDefault();
      latest.current.onToggle();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
