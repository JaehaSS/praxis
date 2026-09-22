import type { Terminal } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";

/** WebGL 렌더러 addon 로드를 시도한다 — `term.open()` 이후에 호출해야 한다.
 * 로드 실패(미지원 환경)나 이후 `webglcontextlost`(GL 컨텍스트 유실) 시 addon을
 * dispose해 xterm 기본(캔버스) 렌더러로 자동 폴백한다. 실패는 조용히 무시한다
 * (터미널 기능에는 영향 없음, 렌더링 성능만 저하). */
export function tryLoadWebgl(term: Terminal): WebglAddon | null {
  try {
    const addon = new WebglAddon();
    addon.onContextLoss(() => {
      disposeWebgl(addon);
    });
    term.loadAddon(addon);
    return addon;
  } catch {
    return null;
  }
}

/** WebGL addon을 정리한다 — 렌더러 초기화가 끝나기 전이거나 GL 컨텍스트가 이미
 * 사라진 상태에서 dispose하면 addon 내부가 throw한다. 그 예외를 그대로 두면
 * `term.dispose()`를 타고 올라가 React 트리 전체가 언마운트되므로 여기서 삼킨다. */
export function disposeWebgl(addon: WebglAddon | null): void {
  if (!addon) return;
  try {
    addon.dispose();
  } catch {
    /* 이미 정리된 GL 컨텍스트 — 렌더러는 GC에 맡긴다 */
  }
}

/** 터미널을 정리한다 — addon dispose 실패가 터미널 정리를 막지 않도록
 * [disposeWebgl]과 함께 사용한다. */
export function disposeTerminal(term: Terminal): void {
  try {
    term.dispose();
  } catch {
    /* addon 정리 중 예외 — 남은 자원은 GC에 맡긴다 */
  }
}
