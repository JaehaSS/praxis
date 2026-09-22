import { createContext, useContext } from "react";
import { LOCAL_HOST, type HostId } from "./transport";

/**
 * 세션에 속하지 않는 화면(파일·메모리·일정·GitHub·Quick Open)이 보는 호스트.
 *
 * 세션은 각자 호스트를 갖는다(`Task.host`). 하지만 이 화면들에는 그런 소유자가 없어
 * 어딘가에서 "어느 머신 것인가"를 정해야 한다. 그 값을 모듈 전역에 두면 마지막 연결이
 * 조용히 결정하게 된다 — ADR 0133이 없애려던 바로 그것이다.
 *
 * 그래서 **App 루트에서 명시적으로 주입하고 사이드바에서 사용자가 고른다.** 컨텍스트인
 * 이유는 프롭 드릴링을 피하기 위해서지 값을 숨기기 위해서가 아니다: 주입은 한 곳이고,
 * 소비는 `useHostScope()`로 드러난다. 나중에 특정 화면만 다른 호스트를 보게 하려면
 * 그 서브트리를 다른 Provider로 감싸면 된다.
 *
 * 기본값이 로컬인 것은 안전한 쪽이다 — Provider 밖(테스트·팝아웃 창)에서는 이 PC를 본다.
 */
const HostScopeContext = createContext<HostId>(LOCAL_HOST);

export const HostScopeProvider = HostScopeContext.Provider;

export function useHostScope(): HostId {
  return useContext(HostScopeContext);
}
