import { BASE } from "./routes";

export type ServiceWorkerState =
  | { kind: "unsupported" }
  | { kind: "registered"; scope: string }
  | { kind: "failed"; reason: string };

let state: ServiceWorkerState = { kind: "unsupported" };

/** 마지막 등록 결과. /m/settings의 푸시 상태 표시가 읽는다. (설계 0013 §8) */
export function serviceWorkerState(): ServiceWorkerState {
  return state;
}

/**
 * Service Worker를 /m/ 스코프로 등록한다.
 * secure context(= tailscale serve의 HTTPS)가 아니면 브라우저가 navigator.serviceWorker를
 * 노출하지 않으므로, 그 경우도 unsupported로 조용히 넘어간다. (설계 0013 §5.1)
 */
export async function registerServiceWorker(): Promise<ServiceWorkerState> {
  if (!("serviceWorker" in navigator)) {
    state = { kind: "unsupported" };
    return state;
  }
  try {
    const registration = await navigator.serviceWorker.register(`${BASE}/sw.js`, {
      scope: `${BASE}/`,
    });
    state = { kind: "registered", scope: registration.scope };
  } catch (error) {
    state = { kind: "failed", reason: error instanceof Error ? error.message : String(error) };
  }
  return state;
}
