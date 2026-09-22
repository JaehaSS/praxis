import { useLayoutEffect, useRef, type RefObject } from "react";
import { createUndoStack, type TextSnapshot } from "../../lib/textarea-undo";

interface Options {
  value: string;
  setValue: (next: string) => void;
  ref: RefObject<HTMLTextAreaElement | null>;
  /** 값이 사는 슬롯의 좌표. 바뀌면 스택을 버린다 — 다른 초안의 문장이 ⌘Z로 새어 들면 안 된다. */
  resetKey?: string | null;
}

/** ⌘Z/Ctrl+Z 되돌리기 · ⇧⌘Z/⌘Y 다시하기. */
function undoIntent(e: React.KeyboardEvent): "undo" | "redo" | null {
  if (!(e.metaKey || e.ctrlKey) || e.altKey) return null;
  const key = e.key.toLowerCase();
  if (key === "y") return "redo";
  if (key !== "z") return null;
  return e.shiftKey ? "redo" : "undo";
}

const caretAtEnd = (value: string): TextSnapshot => ({
  value,
  start: value.length,
  end: value.length,
});

/**
 * controlled textarea에 JS 되돌리기를 붙인다.
 *
 * Edit 메뉴에서 Undo/Redo를 뺀 탓에(Monaco 되돌리기 우선) WKWebView의 textarea는
 * ⌘Z가 동작하지 않는다. 훅을 거치지 않은 값 변경(제출 후 비우기, 멘션 삽입)도
 * 한 단계로 기록해 되돌릴 수 있게 한다.
 */
export function useTextareaUndo({ value, setValue, ref, resetKey = null }: Options) {
  const stackRef = useRef(createUndoStack());
  const lastRef = useRef<TextSnapshot>(caretAtEnd(value));
  const pendingSelRef = useRef<TextSnapshot | null>(null);
  const keyRef = useRef(resetKey);
  // 슬롯 교체는 두 커밋으로 나뉘어 올 수 있다(useSessionDraft는 layout effect에서 값을 바꾼다).
  // 뒤따라오는 값 변경까지 "외부 편집"으로 기록하지 않으려고 표시를 남긴다.
  const swapPendingRef = useRef(false);
  const composeBaseRef = useRef<TextSnapshot | null>(null);

  useLayoutEffect(() => {
    if (resetKey !== keyRef.current) {
      // 값이 아직 옛것이면 교체의 두 번째 커밋이 뒤따라온다 — 그것까지 기록하지 않는다.
      swapPendingRef.current = value === lastRef.current.value;
      keyRef.current = resetKey;
      stackRef.current = createUndoStack();
      lastRef.current = caretAtEnd(value);
      pendingSelRef.current = null;
      return;
    }
    if (value !== lastRef.current.value) {
      const next = caretAtEnd(value);
      if (!swapPendingRef.current) stackRef.current.record(lastRef.current, next, Date.now());
      lastRef.current = next;
    }
    swapPendingRef.current = false;
    const pending = pendingSelRef.current;
    if (!pending) return;
    pendingSelRef.current = null;
    ref.current?.setSelectionRange(pending.start, pending.end);
  }, [value, resetKey, ref]);

  const trackSelection = (el: HTMLTextAreaElement): void => {
    lastRef.current = { ...lastRef.current, start: el.selectionStart, end: el.selectionEnd };
  };

  const onChange = (e: React.ChangeEvent<HTMLTextAreaElement>): void => {
    swapPendingRef.current = false;
    const next: TextSnapshot = {
      value: e.target.value,
      start: e.target.selectionStart,
      end: e.target.selectionEnd,
    };
    // WebKit은 compositionend를 마지막 input보다 먼저 보낼 수 있다 — 값이 그대로면 빈 기록을 남기지 않는다.
    if (next.value === lastRef.current.value) {
      trackSelection(e.target);
      return;
    }
    // 조합 중에는 자모 하나하나가 아니라 조합 전체를 한 단계로 남긴다(compositionend에서 기록).
    if (!composeBaseRef.current) stackRef.current.record(lastRef.current, next, Date.now());
    lastRef.current = next;
    setValue(next.value);
  };

  /** 처리했으면 true — 호출자는 즉시 return한다. */
  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (e.nativeEvent.isComposing) return false;
    const intent = undoIntent(e);
    if (!intent) {
      // 편집 직전의 선택 영역을 잡아 둔다 — 되돌린 뒤 캐럿을 제자리에 놓기 위해.
      trackSelection(e.currentTarget);
      return false;
    }
    e.preventDefault();
    if (e.currentTarget.readOnly || e.currentTarget.disabled) return true;
    const stack = stackRef.current;
    const snapshot =
      intent === "undo" ? stack.undo(lastRef.current) : stack.redo(lastRef.current);
    if (!snapshot) return true;
    const sameValue = snapshot.value === lastRef.current.value;
    lastRef.current = snapshot;
    // 값이 같으면 리렌더가 없다 — layout effect를 기다리면 캐럿이 영영 안 옮겨진다.
    if (sameValue) e.currentTarget.setSelectionRange(snapshot.start, snapshot.end);
    else pendingSelRef.current = snapshot;
    setValue(snapshot.value);
    return true;
  };

  const onSelect = (e: React.SyntheticEvent<HTMLTextAreaElement>): void => {
    trackSelection(e.currentTarget);
  };

  const onCompositionStart = (): void => {
    composeBaseRef.current = lastRef.current;
  };

  const onCompositionEnd = (): void => {
    const base = composeBaseRef.current;
    composeBaseRef.current = null;
    if (!base || base.value === lastRef.current.value) return;
    // 음절마다 조합이 끝나도 묶기 간격 안이면 앞 음절과 한 묶음이 된다.
    stackRef.current.record(base, lastRef.current, Date.now(), "insert");
  };

  return { onChange, onKeyDown, onSelect, onCompositionStart, onCompositionEnd };
}
