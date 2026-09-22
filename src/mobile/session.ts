import { useCallback, useEffect, useRef, useState } from "react";
import { BASE } from "./routes";

// 모바일 세션 부트스트랩 — QR로 받은 일회용 코드를 쿠키 세션으로 교환한다. (설계 0013 §6.1)
// 세션 토큰은 HttpOnly 쿠키라 JS가 읽을 수 없다. 여기서는 "붙었는지"만 알 수 있다.
//
// 폰은 깨어나는 순간 라디오·VPN이 아직 붙는 중이라 첫 요청이 흔히 실패한다. 그 한 번을
// "세션 끊김"으로 단정하면 멀쩡한 세션을 두고 재페어링을 요구하게 된다 — 실제로 그렇게
// 동작했다. 그래서 (a) 일시 실패는 재시도하고 (b) 401만 자격 문제로 취급한다.

/** 페어링 코드는 Runner가 발급하는 32바이트 hex다. 형식이 다르면 시도하지 않는다. */
const CODE_PATTERN = /^[0-9a-f]{64}$/;

/** 한 번이라도 붙었던 기기인지 기억한다. 네트워크 실패와 미페어링을 구분하는 근거다. */
const PAIRED_KEY = "praxis-mobile-paired";

/** 깨어나는 중일 수 있으므로 몇 번은 더 기다려 본다. */
const PROBE_RETRIES = 3;
const PROBE_RETRY_MS = [400, 1200, 2500];

/**
 * `#pair=<code>` 프래그먼트에서 코드를 꺼낸다.
 * 프래그먼트를 쓰는 이유: 쿼리스트링과 달리 서버 로그·Referer에 남지 않는다.
 */
export function readPairingCode(hash: string): string | null {
  const fragment = hash.startsWith("#") ? hash.slice(1) : hash;
  for (const pair of fragment.split("&")) {
    const [key, value] = pair.split("=");
    if (key !== "pair" || !value) continue;
    const code = decodeURIComponent(value);
    return CODE_PATTERN.test(code) ? code : null;
  }
  return null;
}

export type SessionState =
  | { kind: "checking" }
  /** 자격이 있고 Runner가 응답한다. */
  | { kind: "ready" }
  /** 자격이 없다 — QR 페어링이 필요하다. 401로만 도달한다. */
  | { kind: "unpaired" }
  /**
   * Runner에 닿지 못했다. **자격 문제가 아니다.**
   * 전에 붙은 적이 있으면(`everPaired`) 화면을 가리지 않고 배너로만 알린다.
   */
  | { kind: "unreachable"; status?: number; everPaired: boolean };

export function everPaired(): boolean {
  try {
    return globalThis.localStorage?.getItem(PAIRED_KEY) === "1";
  } catch {
    return false;
  }
}

function rememberPaired(paired: boolean): void {
  try {
    if (paired) globalThis.localStorage?.setItem(PAIRED_KEY, "1");
    else globalThis.localStorage?.removeItem(PAIRED_KEY);
  } catch {
    /* 저장소가 막혀 있어도 기능은 계속된다 */
  }
}

/** 표시용 기기 이름. 정확할 필요는 없고, 기기 목록에서 구분만 되면 된다. */
function deviceLabel(): string {
  const agent = navigator.userAgent;
  if (/iPhone/.test(agent)) return "iPhone";
  if (/iPad/.test(agent)) return "iPad";
  if (/Android/.test(agent)) return "Android";
  return "모바일 기기";
}

/** 코드를 세션으로 교환한다. 성공하면 Set-Cookie가 브라우저에 저장된다. */
export async function redeemPairing(code: string): Promise<boolean> {
  try {
    const response = await fetch(`${BASE}/pair`, {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code, label: deviceLabel() }),
    });
    if (response.ok) rememberPaired(true);
    return response.ok;
  } catch {
    return false;
  }
}

/** 단발 조회. 401만 자격 문제이고, 그 외 실패는 전부 도달 실패다. */
async function probeOnce(): Promise<SessionState> {
  try {
    const response = await fetch("/v1/health", { credentials: "same-origin" });
    if (response.ok) {
      rememberPaired(true);
      return { kind: "ready" };
    }
    if (response.status === 401) {
      // 자격이 실제로 무효해진 경우에만 페어링 기억을 지운다.
      rememberPaired(false);
      return { kind: "unpaired" };
    }
    return { kind: "unreachable", status: response.status, everPaired: everPaired() };
  } catch {
    return { kind: "unreachable", everPaired: everPaired() };
  }
}

/**
 * 도달 실패는 몇 번 더 시도한다. 폰이 깨어나는 중이면 첫 시도가 거의 항상 실패하는데,
 * 그걸 결론으로 삼으면 멀쩡한 세션을 두고 재페어링 화면을 띄우게 된다.
 */
export async function probeSession(
  sleep: (ms: number) => Promise<void> = (ms) =>
    new Promise((resolve) => setTimeout(resolve, ms)),
): Promise<SessionState> {
  let last = await probeOnce();
  for (let attempt = 0; attempt < PROBE_RETRIES && last.kind === "unreachable"; attempt += 1) {
    await sleep(PROBE_RETRY_MS[attempt] ?? PROBE_RETRY_MS[PROBE_RETRY_MS.length - 1]);
    last = await probeOnce();
  }
  return last;
}

/** 화면을 가려야 하는 상태인지. 전에 붙은 적 있는 기기의 일시적 도달 실패는 가리지 않는다. */
export function blocksApp(state: SessionState): boolean {
  if (state.kind === "unpaired") return true;
  if (state.kind === "unreachable") return !state.everPaired;
  return false;
}

/**
 * 진입 시 1회: 프래그먼트에 코드가 있으면 교환하고 주소창에서 지운 뒤, 세션 상태를 확인한다.
 * 코드를 남겨두면 새로고침·공유로 재사용이 시도되고 브라우저 이력에도 남는다.
 */
export function useSession(): { state: SessionState; recheck: () => void } {
  const [state, setState] = useState<SessionState>({ kind: "checking" });
  const running = useRef(false);

  const run = useCallback(async (withPairing: boolean) => {
    // 겹쳐 돌면 늦게 끝난 실패가 성공을 덮어쓸 수 있다.
    if (running.current) return;
    running.current = true;
    try {
      if (withPairing) {
        const code = readPairingCode(window.location.hash);
        if (code) {
          await redeemPairing(code);
          window.history.replaceState(null, "", window.location.pathname);
        }
      }
      setState(await probeSession());
    } finally {
      running.current = false;
    }
  }, []);

  const recheck = useCallback(() => {
    void run(false);
  }, [run]);

  useEffect(() => {
    void run(true);
  }, [run]);

  return { state, recheck };
}
