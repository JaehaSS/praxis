import { useSyncExternalStore } from "react";
import { matchRoute, type Route } from "./routes";

// history API 위의 최소 라우터. 경로가 5개뿐이라 외부 라우터 의존성을 두지 않는다.
// 러너가 /m/* 를 SPA fallback으로 서빙하므로 새로고침·딥링크도 그대로 동작한다.

const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  window.addEventListener("popstate", listener);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("popstate", listener);
  };
}

function snapshot(): string {
  return window.location.pathname;
}

/** 현재 경로를 Route로 구독한다. */
export function useRoute(): Route {
  return matchRoute(useSyncExternalStore(subscribe, snapshot));
}

/** pushState/replaceState 후 구독자에게 알린다. 같은 경로면 아무 것도 하지 않는다. */
export function navigate(href: string, options?: { replace?: boolean }): void {
  if (href === window.location.pathname) return;
  if (options?.replace) window.history.replaceState(null, "", href);
  else window.history.pushState(null, "", href);
  notify();
}

/** 앵커 기본 동작을 가로채 SPA 전환으로 바꾼다. 새 탭 열기(수식키·중클릭)는 통과시킨다. */
export function linkProps(href: string): {
  href: string;
  onClick: (event: React.MouseEvent<HTMLAnchorElement>) => void;
} {
  return {
    href,
    onClick: (event) => {
      if (event.defaultPrevented) return;
      if (event.button !== 0) return;
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      navigate(href);
    },
  };
}
