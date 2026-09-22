// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  replOpen: vi.fn(),
  /** 마운트된 터미널이 받은 채널 — 도크의 open 결정을 여기서 관찰한다. */
  channels: [] as Array<{ open: (id: number, cols: number, rows: number) => Promise<boolean> }>,
  opened: vi.fn(),
}));

vi.mock("../../lib/ipc", () => ({
  replOpen: h.replOpen,
  replWrite: vi.fn(),
  replResize: vi.fn(),
  replReplay: vi.fn(),
  replDetach: vi.fn(),
}));

// 실제 xterm은 canvas를 만진다 — 도크가 어떤 채널로 열려고 하는지만 본다.
vi.mock("./ShellTerminal", () => ({
  ShellTerminal: ({
    taskId,
    channel,
  }: {
    taskId: number;
    channel: { open: (id: number, cols: number, rows: number) => Promise<boolean> };
  }) => {
    h.channels.push(channel);
    return <div data-repl={taskId} />;
  },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const { ReplDock } = await import("./ReplDock");

let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  h.replOpen.mockReset();
  h.channels.length = 0;
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const mount = (available = true) =>
  act(() => {
    root.render(
      <ReplDock taskId={7} available={available} label="wt-7" onOpened={h.opened} onClose={() => {}} />,
    );
  });

describe("Python 콘솔 도크", () => {
  it("로컬 작업이면 repl 채널로 터미널을 띄운다", async () => {
    h.replOpen.mockResolvedValue({ status: "started", python: null });
    await mount();
    expect(host.querySelector("[data-repl='7']")).not.toBeNull();
    await expect(h.channels[0].open(7, 80, 24)).resolves.toBe(false);
    expect(h.replOpen).toHaveBeenCalledWith(7, 80, 24, false);
  });

  it("이미 열려 있던 콘솔이면 재사용을 알린다", async () => {
    h.replOpen.mockResolvedValue({ status: "existed", python: null });
    await mount();
    await expect(h.channels[0].open(7, 80, 24)).resolves.toBe(true);
  });

  it("ipython이 없으면 터미널 대신 설치 제안을 띄우고, 승인하면 install로 다시 연다", async () => {
    h.replOpen.mockResolvedValue({ status: "missing", python: "/usr/bin/python3" });
    await mount();
    await act(async () => {
      await expect(h.channels[0].open(7, 80, 24)).rejects.toThrow("ipython");
    });
    expect(host.querySelector("[data-repl]")).toBeNull();
    const button = host.querySelector("button.bg-primary") as HTMLButtonElement | null;
    expect(button?.textContent).toContain("/usr/bin/python3 -m pip install ipython");

    h.replOpen.mockResolvedValue({ status: "started", python: null });
    await act(async () => button?.click());
    expect(host.querySelector("[data-repl='7']")).not.toBeNull();
    await h.channels[h.channels.length - 1].open(7, 80, 24);
    expect(h.replOpen).toHaveBeenLastCalledWith(7, 80, 24, true);
  });

  it("python3도 없으면 설치 버튼 없이 이유만 남긴다", async () => {
    h.replOpen.mockResolvedValue({ status: "missing", python: null });
    await mount();
    await act(async () => {
      await h.channels[0].open(7, 80, 24).catch(() => undefined);
    });
    expect(host.querySelector("button.bg-primary")).toBeNull();
    expect(host.textContent).toContain("python3도 없어");
  });

  it("원격 작업이면 콘솔 대신 이유를 알린다", async () => {
    await mount(false);
    expect(host.querySelector("[data-repl]")).toBeNull();
    expect(host.textContent).toContain("로컬 작업에서만");
  });
});
