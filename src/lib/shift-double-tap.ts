// Shift 더블탭 감지 — IntelliJ의 "Search Everywhere".
//
// 순수 로직으로 떼어 둔 이유는 취소 조건이 셋이나 되기 때문이다. 컴포넌트 안에 두면
// 그중 하나가 빠져도 아무도 모른다.

/** 두 번째 탭이 이 안에 들어와야 발동한다. */
export const SHIFT_DOUBLE_TAP_MS = 300;

export interface ShiftTapState {
  /** 마지막 Shift keyup 시각. 0이면 대기 없음. */
  lastUp: number;
}

export interface KeyLike {
  key: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  isComposing?: boolean;
}

/**
 * keydown 처리 — **Shift가 아닌 키가 끼면 취소한다.**
 * `Shift+A`를 입력하는 동안의 Shift 두 번은 의도가 아니다.
 */
export function onKeyDown(state: ShiftTapState, e: KeyLike): ShiftTapState {
  return e.key === "Shift" ? state : { lastUp: 0 };
}

/** keyup 처리 — 발동해야 하면 `fire: true`. */
export function onKeyUp(
  state: ShiftTapState,
  e: KeyLike,
  now: number,
): { next: ShiftTapState; fire: boolean } {
  if (e.key !== "Shift") return { next: state, fire: false };
  // 다른 수식자와 함께면 무시한다 — `⇧⌘P` 같은 기존 조합을 잡아먹지 않는다.
  if (e.metaKey || e.ctrlKey || e.altKey) return { next: { lastUp: 0 }, fire: false };
  // IME 조합 중이면 무시한다 — 한글 입력 도중 오발동을 막는다.
  if (e.isComposing) return { next: { lastUp: 0 }, fire: false };

  if (state.lastUp > 0 && now - state.lastUp < SHIFT_DOUBLE_TAP_MS) {
    return { next: { lastUp: 0 }, fire: true };
  }
  return { next: { lastUp: now }, fire: false };
}
