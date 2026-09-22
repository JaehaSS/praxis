import { useEffect, useRef, useState, type KeyboardEvent } from "react";

interface Options {
  /** 메뉴가 열려 있는가. 여닫는 상태는 호출자가 쥔다. */
  open: boolean;
  /** 지금 보이는 행 수 — ↑↓의 하한·상한. */
  count: number;
  /** 커서를 되맞출 자리. 호출자가 매 렌더 계산한다. */
  initial: number;
  /**
   * 되맞추는 시점. `"open"`은 열 때만 — 질의가 바뀌어도 커서를 되돌리지 않는다(브랜치).
   * `"open+query"`는 질의가 바뀔 때마다 `initial`로 간다(레포·모델).
   */
  resetOn: "open" | "open+query";
  query: string;
  /** Enter. 커서를 클램프하지 않으므로 행이 실재하는지는 호출자가 가른다. */
  onCommit: (index: number) => void;
  /** Esc — 질의를 먼저 비우는 2단계는 두지 않는다. */
  onClose: () => void;
}

interface Cursor {
  cursor: number;
  setCursor: React.Dispatch<React.SetStateAction<number>>;
  inputRef: React.RefObject<HTMLInputElement | null>;
  listRef: React.RefObject<HTMLDivElement | null>;
  onKeyDown: (e: KeyboardEvent<HTMLInputElement>) => void;
}

/**
 * 검색 피커의 커서·키보드 — 열 때 포커스, ↑↓ 비순환, Enter 확정, 1단 Esc, `scrollIntoView(nearest)`.
 *
 * **커서를 클램프하지 않는다.** 열 때 맞춘 커서가 필터로 범위 밖에 남으면 그대로 내보내고,
 * 확정 여부는 호출자가 정한다 — 클램프하면 브랜치의 "Enter 무동작"이 "다른 브랜치 확정"으로
 * 조용히 바뀐다(설계 0062 §4.2 D-2).
 */
export function usePickerCursor({
  open,
  count,
  initial,
  resetOn,
  query,
  onCommit,
  onClose,
}: Options): Cursor {
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const wasOpen = useRef(false);
  // 되맞출 자리는 최신 렌더의 값이어야 한다 — 의존성에 넣으면 매 렌더 커서가 되돌아간다.
  const initialRef = useRef(initial);
  initialRef.current = initial;

  useEffect(() => {
    if (!open) {
      wasOpen.current = false;
      return;
    }
    const justOpened = !wasOpen.current;
    wasOpen.current = true;
    if (justOpened) inputRef.current?.focus();
    if (justOpened || resetOn === "open+query") setCursor(initialRef.current);
  }, [open, query, resetOn]);

  // 커서가 접힌 자리에 있으면 보이는 데까지만 끌어온다(jsdom에는 scrollIntoView가 없다).
  useEffect(() => {
    if (!open) return;
    const row = listRef.current?.querySelector<HTMLElement>('[data-cursor="true"]');
    row?.scrollIntoView?.({ block: "nearest" });
  }, [cursor, open, count]);

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => Math.min(c + 1, count - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => Math.max(c - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      onCommit(cursor);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  };

  return { cursor, setCursor, inputRef, listRef, onKeyDown };
}
