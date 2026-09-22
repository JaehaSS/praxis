/**
 * @vitest-environment jsdom
 *
 * 세션 판정 — 일시적 도달 실패와 자격 만료를 섞으면 멀쩡한 세션을 두고 재페어링을 요구한다.
 * 2026-07-26 실제로 그렇게 동작했다: 서버 세션은 29일 남아 있는데 폰이 "다시 연결" 화면에
 * 갇혔고, 23분간 재시도조차 하지 않았다.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { blocksApp, everPaired, probeSession, type SessionState } from "./session";

const noSleep = () => Promise.resolve();

function mockFetch(...responses: (Response | Error)[]) {
  const fn = vi.fn();
  for (const response of responses) {
    if (response instanceof Error) fn.mockRejectedValueOnce(response);
    else fn.mockResolvedValueOnce(response);
  }
  // 지정한 것보다 더 부르면 마지막 응답을 반복한다.
  const last = responses[responses.length - 1];
  if (last instanceof Error) fn.mockRejectedValue(last);
  else fn.mockResolvedValue(last);
  vi.stubGlobal("fetch", fn);
  return fn;
}

const ok = () => new Response("{}", { status: 200 });
const status = (code: number) => new Response("", { status: code });

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("probeSession", () => {
  it("성공하면 페어링 사실을 기억한다", async () => {
    mockFetch(ok());
    expect(await probeSession(noSleep)).toEqual({ kind: "ready" });
    expect(everPaired()).toBe(true);
  });

  it("일시적 실패는 재시도해서 회복한다", async () => {
    // 폰이 깨어나는 중이면 첫 요청이 거의 항상 실패한다. 그걸 결론으로 삼으면 안 된다.
    const fetchMock = mockFetch(new Error("network"), new Error("network"), ok());
    expect(await probeSession(noSleep)).toEqual({ kind: "ready" });
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it("끝까지 못 닿으면 도달 실패로 남는다 — 자격 문제로 바꾸지 않는다", async () => {
    localStorage.setItem("praxis-mobile-paired", "1");
    mockFetch(new Error("network"));
    const state = await probeSession(noSleep);
    expect(state.kind).toBe("unreachable");
    expect((state as Extract<SessionState, { kind: "unreachable" }>).everPaired).toBe(true);
    // 도달 실패로 페어링 기억을 지우면, 회복된 뒤에도 재페어링을 요구하게 된다.
    expect(everPaired()).toBe(true);
  });

  it("502는 Runner 다운이지 세션 만료가 아니다", async () => {
    localStorage.setItem("praxis-mobile-paired", "1");
    mockFetch(status(502));
    const state = await probeSession(noSleep);
    expect(state).toMatchObject({ kind: "unreachable", status: 502 });
    expect(everPaired()).toBe(true);
  });

  it("401만 자격 만료로 보고 기억을 지운다", async () => {
    localStorage.setItem("praxis-mobile-paired", "1");
    mockFetch(status(401));
    expect(await probeSession(noSleep)).toEqual({ kind: "unpaired" });
    expect(everPaired()).toBe(false);
  });

  it("401은 재시도하지 않는다", async () => {
    const fetchMock = mockFetch(status(401));
    await probeSession(noSleep);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

describe("blocksApp", () => {
  it("전에 붙은 적 있으면 도달 실패로 화면을 가리지 않는다", () => {
    // 가리면 낡은 화면조차 못 보고, 사용자는 세션이 끊긴 줄 안다.
    expect(blocksApp({ kind: "unreachable", everPaired: true })).toBe(false);
  });

  it("한 번도 안 붙은 기기는 가린다", () => {
    expect(blocksApp({ kind: "unreachable", everPaired: false })).toBe(true);
  });

  it("자격 만료는 항상 가린다", () => {
    expect(blocksApp({ kind: "unpaired" })).toBe(true);
  });

  it("정상·확인중은 가리지 않는다", () => {
    expect(blocksApp({ kind: "ready" })).toBe(false);
    expect(blocksApp({ kind: "checking" })).toBe(false);
  });
});
