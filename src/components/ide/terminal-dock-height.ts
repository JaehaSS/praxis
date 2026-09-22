import {
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";

/** 프롬프트 한 줄과 헤더가 함께 보이는 최소 높이. 이보다 낮으면 터미널이 아니라 띠가 된다. */
const MIN_HEIGHT = 120;
/** 도크를 최대로 올려도 에디터에 남겨두는 높이. */
export const MIN_EDITOR_HEIGHT = 200;
const DEFAULT_HEIGHT = 260;
/** window가 없는 환경(테스트·SSR)에서 상한 계산의 기준으로 삼는 창 높이. */
const FALLBACK_VIEWPORT = 900;
const HEIGHT_KEY = "praxis-terminal-dock-height";

function viewportHeight(): number {
  return typeof window === "undefined" ? FALLBACK_VIEWPORT : window.innerHeight;
}

/**
 * 창 높이에서 에디터 최소 높이를 뺀 값이 도크 상한. 창이 아주 낮으면 MIN_HEIGHT가 이긴다.
 * 창 크기에 따라 달라지므로 저장·복원 시점마다 다시 계산한다(사이드 패널 폭과 같은 규칙).
 */
export function maxDockHeight(viewport: number = viewportHeight()): number {
  return Math.max(MIN_HEIGHT, viewport - MIN_EDITOR_HEIGHT);
}

function clampHeight(height: number, viewport?: number): number {
  return Math.min(maxDockHeight(viewport), Math.max(MIN_HEIGHT, height));
}

export interface DockHeightStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function readStoredDockHeight(storage: DockHeightStorage, viewport?: number): number {
  const raw = storage.getItem(HEIGHT_KEY);
  if (raw == null) return DEFAULT_HEIGHT;
  const height = Number(raw);
  return Number.isFinite(height) ? clampHeight(height, viewport) : DEFAULT_HEIGHT;
}

export function writeStoredDockHeight(
  storage: DockHeightStorage,
  height: number,
  viewport?: number,
): void {
  storage.setItem(HEIGHT_KEY, String(clampHeight(height, viewport)));
}

function readHeight(): number {
  return typeof localStorage === "undefined" ? DEFAULT_HEIGHT : readStoredDockHeight(localStorage);
}

function saveHeight(height: number): void {
  if (typeof localStorage !== "undefined") writeStoredDockHeight(localStorage, height);
}

interface DockHeight {
  height: number;
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLDivElement>) => void;
}

/** 하단 터미널 도크의 높이 상태 — 위쪽 경계를 끌어 조절하고 창 전체에서 하나의 값을 공유한다. */
export function useTerminalDockHeight(): DockHeight {
  const [height, setHeight] = useState<number>(() => readHeight());

  const resizeTo = (next: number): number => {
    const height = clampHeight(next);
    setHeight(height);
    return height;
  };
  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>): void => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = height;
    let finalHeight = height;
    const move = (moveEvent: PointerEvent) => {
      // 위로 끌수록 커진다 — 도크가 아래에 붙어 있으므로 부호가 폭 조절과 반대다.
      finalHeight = resizeTo(startHeight + startY - moveEvent.clientY);
    };
    const stop = () => {
      saveHeight(finalHeight);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  };
  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>): void => {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const next = resizeTo(height + (event.key === "ArrowUp" ? 24 : -24));
    saveHeight(next);
  };
  return { height, onPointerDown, onKeyDown };
}
