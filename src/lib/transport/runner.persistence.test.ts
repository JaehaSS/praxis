/**
 * @vitest-environment jsdom
 *
 * subscribeEvents의 커서 영속 배선 검증. (설계 0013 §7.3)
 * runner.test.ts는 node 환경이라 localStorage가 없어 이 경로가 타지 않는다.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RunnerTransport, type RunnerWebSocket } from "./runner";

const token = "a".repeat(64);
const endpoint = "http://127.0.0.1:49123";

function harness() {
  const sockets: RunnerWebSocket[] = [];
  const factory = vi.fn((_url: string, _protocols: string[]) => {
    const socket: RunnerWebSocket = { close: vi.fn(), onmessage: null, onerror: null, onclose: null };
    sockets.push(socket);
    return socket;
  });
  const transport = new RunnerTransport({ endpoint, pairingToken: token }, fetch, factory);
  return { sockets, factory, transport };
}

function event(sequence: number) {
  return {
    data: JSON.stringify({ sequence, task_id: 7, ts: 1, kind: "output", detail: "x" }),
  } as MessageEvent<string>;
}

beforeEach(() => {
  localStorage.clear();
});

describe("커서 영속", () => {
  it("앱을 다시 띄워도 마지막 sequence부터 이어 받는다", () => {
    // PWA는 백그라운드에서 수시로 죽는다. 커서가 메모리에만 있으면 매번 처음부터 replay한다.
    const first = harness();
    const stop = first.transport.subscribeEvents(0, vi.fn(), vi.fn());
    first.sockets[0].onmessage?.(event(42));
    stop();

    const second = harness();
    second.transport.subscribeEvents(0, vi.fn(), vi.fn());
    expect(second.factory).toHaveBeenCalledWith(
      `ws://127.0.0.1:49123/v1/events/live?after=42`,
      ["praxis", token],
    );
  });

  it("호출자가 더 뒤를 요청하면 그쪽을 존중한다", () => {
    const first = harness();
    first.transport.subscribeEvents(0, vi.fn(), vi.fn());
    first.sockets[0].onmessage?.(event(10));

    const second = harness();
    second.transport.subscribeEvents(99, vi.fn(), vi.fn());
    expect(second.factory).toHaveBeenCalledWith(
      `ws://127.0.0.1:49123/v1/events/live?after=99`,
      ["praxis", token],
    );
  });

  it("커서보다 앞선 이벤트는 배달되지 않는다 — 스트림은 복원 채널이 아니다", () => {
    // 이 성질 때문에 `subscribeEvents(0, …)`로는 이력을 되살릴 수 없다. 커서는 endpoint당
    // 하나이고 이벤트마다 전진하므로, 이미 끝난 작업의 출력은 항상 그 아래에 깔린다.
    // 화면이 이력을 원하면 작업 단위 `taskOutput`으로 읽어야 한다(TerminalView·대화 모드 공통).
    const first = harness();
    first.transport.subscribeEvents(0, vi.fn(), vi.fn());
    first.sockets[0].onmessage?.(event(42));

    const second = harness();
    const onEvent = vi.fn();
    second.transport.subscribeEvents(0, onEvent, vi.fn());
    second.sockets[0].onmessage?.(event(7));

    expect(onEvent).not.toHaveBeenCalled();
  });

  it("endpoint가 다르면 커서를 섞지 않는다", () => {
    const first = harness();
    first.transport.subscribeEvents(0, vi.fn(), vi.fn());
    first.sockets[0].onmessage?.(event(42));

    const otherFactory = vi.fn((_url: string, _protocols: string[]) => {
      return { close: vi.fn(), onmessage: null, onerror: null, onclose: null } as RunnerWebSocket;
    });
    new RunnerTransport(
      { endpoint: "http://127.0.0.1:59999", pairingToken: token },
      fetch,
      otherFactory,
    ).subscribeEvents(0, vi.fn(), vi.fn());
    expect(otherFactory).toHaveBeenCalledWith(
      "ws://127.0.0.1:59999/v1/events/live?after=0",
      ["praxis", token],
    );
  });

  it("watermark가 커서보다 작으면 0부터 다시 붙는다", () => {
    // Runner DB가 재생성되면 sequence가 되감긴다. 커서를 붙들면 이후 이벤트를 전부 놓친다.
    const first = harness();
    first.transport.subscribeEvents(0, vi.fn(), vi.fn());
    first.sockets[0].onmessage?.(event(500));

    const second = harness();
    second.transport.subscribeEvents(0, vi.fn(), vi.fn());
    expect(second.factory).toHaveBeenNthCalledWith(
      1,
      "ws://127.0.0.1:49123/v1/events/live?after=500",
      ["praxis", token],
    );

    second.sockets[0].onmessage?.({
      data: JSON.stringify({ kind: "watermark", sequence: 3 }),
    } as MessageEvent<string>);

    expect(second.factory).toHaveBeenNthCalledWith(
      2,
      "ws://127.0.0.1:49123/v1/events/live?after=0",
      ["praxis", token],
    );
    // 되감긴 상태를 저장해야 다음 기동도 처음부터 받는다.
    expect(localStorage.getItem(`praxis-runner-seq:${endpoint}`)).toBe("0");
  });

  it("깨워도 소켓이 두 개가 되지 않는다", () => {
    // close()가 onclose를 타면 재연결이 예약되고, 즉시 connect까지 하면 중복 구독이 된다.
    const { sockets, factory, transport } = harness();
    transport.subscribeEvents(0, vi.fn(), vi.fn());
    expect(factory).toHaveBeenCalledTimes(1);

    window.dispatchEvent(new Event("online"));

    expect(factory).toHaveBeenCalledTimes(2);
    expect(sockets[0].onclose).toBeNull();
    expect(sockets[0].close).toHaveBeenCalledOnce();
  });
});
