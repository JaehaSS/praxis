// 활성 테마 구독 훅. themes.ts는 보조 창·부팅 스크립트도 쓰므로 React 의존을 두지 않고
// 여기서만 붙인다.
import { useSyncExternalStore } from "react";
import { getActiveTheme, subscribeTheme, type Theme } from "./themes";

/**
 * 테마는 CSS 변수로 대부분 자동 반영된다. 이 훅이 필요한 곳은 **CSS를 읽지 못하는 소비자**
 * (xterm·Monaco)와 테마 자체를 고르는 UI뿐이다.
 */
export function useTheme(): Theme {
  return useSyncExternalStore(subscribeTheme, getActiveTheme);
}
