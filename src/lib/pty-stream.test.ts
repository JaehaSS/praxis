import { describe, expect, it, vi } from "vitest";
import { attachPtyStream, ptyEventNames } from "./pty-stream";

type Handler<T> = (event: { payload: T }) => void;

describe("attachPtyStream", () => {
  it("선전송(replay) 후 라이브 스트림을 구독한다(순서 고정)", async () => {
    const calls: string[] = [];
    const fetchReplay = vi.fn(async () => {
      calls.push("replay");
      return btoa("hello ");
    });
    const listenFn = vi.fn(async (event: string) => {
      calls.push(`listen:${event}`);
      return () => calls.push(`unlisten:${event}`);
    });
    const onData = vi.fn();

    await attachPtyStream({
      id: 1,
      outputEvent: "pty://output",
      exitEvent: "pty://exit",
      fetchReplay,
      onData,
      onExit: vi.fn(),
      listenFn: listenFn as never,
    });

    expect(calls).toEqual(["replay", "listen:pty://output", "listen:pty://exit"]);
    expect(onData).toHaveBeenCalledTimes(1);
  });

  it("구독하는 이벤트명이 세션 id로 갈린다", async () => {
    const listened: string[] = [];
    const listenFn = vi.fn(async (event: string) => {
      listened.push(event);
      return () => {};
    });

    await attachPtyStream({
      id: 42,
      ...ptyEventNames("shell", 42),
      fetchReplay: async () => "",
      onData: vi.fn(),
      onExit: vi.fn(),
      listenFn: listenFn as never,
    });

    expect(listened).toEqual(["shell://output/42", "shell://exit/42"]);
  });

  it("빈 replay는 onData를 호출하지 않는다", async () => {
    const listenFn = vi.fn(async () => () => {});
    const onData = vi.fn();

    await attachPtyStream({
      id: 1,
      outputEvent: "pty://output",
      exitEvent: "pty://exit",
      fetchReplay: async () => "",
      onData,
      onExit: vi.fn(),
      listenFn: listenFn as never,
    });

    expect(onData).not.toHaveBeenCalled();
  });

  it("replay 조회 실패 시 빈 replay로 취급하고 라이브 구독은 계속 진행한다", async () => {
    const listenFn = vi.fn(async () => () => {});
    const onData = vi.fn();

    const cleanup = await attachPtyStream({
      id: 1,
      outputEvent: "pty://output",
      exitEvent: "pty://exit",
      fetchReplay: async () => {
        throw new Error("세션 없음");
      },
      onData,
      onExit: vi.fn(),
      listenFn: listenFn as never,
    });

    expect(onData).not.toHaveBeenCalled();
    expect(listenFn).toHaveBeenCalledTimes(2);
    cleanup();
  });

  it("라이브 output 이벤트는 id로 필터링되고 base64가 디코드된다", async () => {
    let outputHandler: Handler<{ id: number; data: string }> | undefined;
    const listenFn = vi.fn(async (event: string, handler: unknown) => {
      if (event === "pty://output") outputHandler = handler as Handler<{ id: number; data: string }>;
      return () => {};
    });
    const onData = vi.fn();

    await attachPtyStream({
      id: 42,
      outputEvent: "pty://output",
      exitEvent: "pty://exit",
      fetchReplay: async () => "",
      onData,
      onExit: vi.fn(),
      listenFn: listenFn as never,
    });

    outputHandler?.({ payload: { id: 99, data: btoa("nope") } });
    expect(onData).not.toHaveBeenCalled();

    outputHandler?.({ payload: { id: 42, data: btoa("hi") } });
    expect(onData).toHaveBeenCalledTimes(1);
    expect(new TextDecoder().decode(onData.mock.calls[0][0])).toBe("hi");
  });

  it("id가 일치하는 exit 이벤트는 onExit(code)를 호출한다", async () => {
    let exitHandler: Handler<{ id: number; code: number }> | undefined;
    const listenFn = vi.fn(async (event: string, handler: unknown) => {
      if (event === "pty://exit") exitHandler = handler as Handler<{ id: number; code: number }>;
      return () => {};
    });
    const onExit = vi.fn();

    await attachPtyStream({
      id: 7,
      outputEvent: "pty://output",
      exitEvent: "pty://exit",
      fetchReplay: async () => "",
      onData: vi.fn(),
      onExit,
      listenFn: listenFn as never,
    });

    exitHandler?.({ payload: { id: 8, code: 1 } });
    expect(onExit).not.toHaveBeenCalled();

    exitHandler?.({ payload: { id: 7, code: 130 } });
    expect(onExit).toHaveBeenCalledWith(130);
  });

  it("cleanup은 두 리스너를 모두 해제한다", async () => {
    const unOut = vi.fn();
    const unExit = vi.fn();
    const listenFn = vi.fn(async (event: string) => (event === "pty://output" ? unOut : unExit));

    const cleanup = await attachPtyStream({
      id: 1,
      outputEvent: "pty://output",
      exitEvent: "pty://exit",
      fetchReplay: async () => "",
      onData: vi.fn(),
      onExit: vi.fn(),
      listenFn: listenFn as never,
    });

    cleanup();
    expect(unOut).toHaveBeenCalledTimes(1);
    expect(unExit).toHaveBeenCalledTimes(1);
  });
});
