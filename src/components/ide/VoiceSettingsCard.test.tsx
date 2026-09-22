// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OHR_PRESET, VoiceSettingsCard } from "./VoiceSettingsCard";
import type { VoiceServerStatus, VoiceSettings } from "../../lib/ipc";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SETTINGS: VoiceSettings = {
  base_url: "http://127.0.0.1:8080/v1",
  model: "whisper-1",
  api_key: "key",
  language: "ko",
  hotkey_command: "Alt+Space",
  hotkey_dictation: "Alt+D",
};

const STOPPED: VoiceServerStatus = {
  installed: true,
  binary: "/opt/homebrew/bin/ohr",
  running: false,
  pid: null,
  port: null,
};
const RUNNING: VoiceServerStatus = { ...STOPPED, running: true, pid: 4242, port: 11434 };

let serverStatus: VoiceServerStatus = STOPPED;
/** 기동 응답만 테스트마다 갈아 끼운다 — 실패·보류를 같은 목 한 벌로 표현하려고. */
let startResult: () => Promise<VoiceServerStatus> = async () => RUNNING;
let root: Root | null = null;
let host: HTMLDivElement | null = null;

beforeEach(() => {
  serverStatus = STOPPED;
  startResult = async () => RUNNING;
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === "voice_settings_get") return SETTINGS;
    if (command === "voice_server_status") return serverStatus;
    if (command === "voice_server_start") return startResult();
    if (command === "voice_server_stop") return STOPPED;
    return undefined;
  });
});
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.clearAllMocks();
});

/** 마운트 뒤 초기 조회 두 건(설정·서버 상태)이 반영될 때까지 흘린다. */
async function mount() {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<VoiceSettingsCard />));
  await act(async () => {});
}

const button = (label: string) =>
  [...host!.querySelectorAll("button")].find((b) => b.textContent?.trim() === label);

const inputFor = (label: string) =>
  [...host!.querySelectorAll("label")]
    .find((l) => l.firstElementChild?.textContent === label)
    ?.querySelector("input") as HTMLInputElement;

describe("VoiceSettingsCard", () => {
  it("바이너리가 없으면 시작을 막고 runbook 을 가리킨다", async () => {
    serverStatus = { installed: false, binary: null, running: false, pid: null, port: null };
    await mount();
    expect(host?.textContent).toContain("설치 안 됨");
    expect(button("서버 시작")?.disabled).toBe(true);
  });

  it("화면의 현재 값으로 서버를 띄우고 실행 중으로 바뀐다", async () => {
    await mount();
    expect(button("서버 시작")?.disabled).toBe(false);
    await act(async () => button("서버 시작")?.click());
    expect(mocks.invoke).toHaveBeenCalledWith("voice_server_start", { settings: SETTINGS });
    expect(button("서버 중지")).toBeDefined();
    expect(host?.textContent).toContain("실행 중 · 127.0.0.1:11434 · pid 4242");
  });

  it("프리셋은 칸만 채우고 저장하지 않는다", async () => {
    await mount();
    await act(async () => button("ohr 프리셋 적용")?.click());
    expect(inputFor("서버 주소").value).toBe(OHR_PRESET.base_url);
    expect(inputFor("언어").value).toBe(OHR_PRESET.language);
    expect(mocks.invoke).not.toHaveBeenCalledWith("voice_settings_set", expect.anything());
  });

  it("기동 실패는 그 문구를 그대로 보여주고 버튼을 되돌린다", async () => {
    const message = "서버가 바로 종료됐습니다 (exit status: 1)";
    startResult = () => Promise.reject(message);
    await mount();
    await act(async () => button("서버 시작")?.click());
    expect(host?.querySelector(".text-status-failed")?.textContent).toContain(message);
    expect(button("서버 시작")).toBeDefined();
  });

  it("실행 중이면 중지를 눌러 내린다", async () => {
    serverStatus = RUNNING;
    await mount();
    await act(async () => button("서버 중지")?.click());
    expect(mocks.invoke).toHaveBeenCalledWith("voice_server_stop");
    expect(button("서버 시작")).toBeDefined();
  });

  it("기동을 기다리는 동안 저장·연결 테스트를 잠근다", async () => {
    let release: (status: VoiceServerStatus) => void = () => {};
    startResult = () => new Promise<VoiceServerStatus>((resolve) => (release = resolve));
    await mount();
    await act(async () => button("서버 시작")?.click());
    expect(button("저장")?.disabled).toBe(true);
    expect(button("연결 테스트")?.disabled).toBe(true);
    await act(async () => release(RUNNING));
    expect(button("저장")?.disabled).toBe(false);
  });
});
