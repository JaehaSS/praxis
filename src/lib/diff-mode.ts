import type { DiffMode } from "../components/DiffPresentation";

/** 통합·분할 선택은 화면을 넘어 이어진다. 키는 탭이 생기기 전부터 쓰던 것 그대로다. */
const MODE_KEY = "praxis:diff-mode";

export function storedDiffMode(): DiffMode {
  try {
    return window.localStorage.getItem(MODE_KEY) === "split" ? "split" : "unified";
  } catch {
    return "unified";
  }
}

export function storeDiffMode(mode: DiffMode): void {
  try {
    window.localStorage.setItem(MODE_KEY, mode);
  } catch {
    // private mode 등 storage 거부 시 현재 세션 상태만 유지한다.
  }
}
