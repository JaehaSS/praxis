// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Event, EventCallback } from "@tauri-apps/api/event";
import { EDITOR_READY_EVENT, EDITOR_WINDOW_LABEL, PROJECT_EDITOR_READY_EVENT } from "./editor-window-events";

const mocks = vi.hoisted(() => {
  const listeners = new Map<string, EventCallback<unknown>>();
  return {
    listeners,
    emitTo: vi.fn(async (): Promise<void> => undefined),
    listen: vi.fn(async <T,>(event: string, handler: EventCallback<T>): Promise<() => void> => {
      listeners.set(event, handler as EventCallback<unknown>);
      return () => {
        listeners.delete(event);
      };
    }),
  };
});

vi.mock("@tauri-apps/api/event", () => ({ emitTo: mocks.emitTo, listen: mocks.listen }));

const { startThemeBroadcast, startThemeFollower, THEME_CHANGED_EVENT } = await import("./theme-sync");
const { applyTheme, DEFAULT_THEME_ID, getActiveTheme, getTheme } = await import("./themes");
type Theme = ReturnType<typeof getTheme>;

/** 리스너가 테스트 사이에 새지 않도록 각 테스트가 돌려받는 정리 함수. */
let stop: (() => void) | null = null;

/** 등록된 핸들러에 이벤트를 흘려보낸다 — Tauri가 창 너머에서 하는 일의 대역. */
function deliver(event: string, payload: unknown): void {
  const handler = mocks.listeners.get(event);
  if (!handler) throw new Error(`${event} 리스너가 없다`);
  handler({ event, id: 1, payload } as Event<unknown>);
}

const LIGHT = "praxis-light";

beforeEach(() => {
  mocks.listeners.clear();
  mocks.emitTo.mockClear();
  mocks.listen.mockClear();
  localStorage.clear();
  applyTheme(DEFAULT_THEME_ID);
});

afterEach(() => {
  stop?.();
  stop = null;
});

describe("메인 창 브로드캐스트", () => {
  it("테마를 바꾸면 활성 테마가 에디터 창으로 간다", async () => {
    stop = startThemeBroadcast();
    mocks.emitTo.mockClear();

    applyTheme(LIGHT);

    expect(mocks.emitTo).toHaveBeenCalledWith(
      EDITOR_WINDOW_LABEL,
      THEME_CHANGED_EVENT,
      getTheme(LIGHT),
    );
  });

  it("id가 아니라 파생 토큰까지 실어 보낸다 — 받는 창은 레지스트리를 뒤지지 않는다", () => {
    stop = startThemeBroadcast();

    const [, , payload] = mocks.emitTo.mock.calls[0] as unknown as [string, string, Theme];
    expect(payload.tokens.bg).toBe(getTheme(DEFAULT_THEME_ID).tokens.bg);
  });

  it("창이 준비를 알리면 현재 테마를 다시 준다", async () => {
    stop = startThemeBroadcast();
    await Promise.resolve();
    applyTheme(LIGHT);
    mocks.emitTo.mockClear();

    deliver(EDITOR_READY_EVENT, null);

    expect(mocks.emitTo).toHaveBeenCalledWith(
      EDITOR_WINDOW_LABEL,
      THEME_CHANGED_EVENT,
      getTheme(LIGHT),
    );
  });

  it("준비된 프로젝트 창에도 이후 테마 변경을 보낸다", async () => {
    stop = startThemeBroadcast();
    await Promise.resolve();
    deliver(PROJECT_EDITOR_READY_EVENT, "project-editor-7");
    mocks.emitTo.mockClear();

    applyTheme(LIGHT);

    expect(mocks.emitTo).toHaveBeenCalledWith("project-editor-7", THEME_CHANGED_EVENT, getTheme(LIGHT));
  });

  it("멈춘 뒤에는 보내지 않는다", () => {
    startThemeBroadcast()();
    mocks.emitTo.mockClear();

    applyTheme(LIGHT);

    expect(mocks.emitTo).not.toHaveBeenCalled();
  });
});

describe("에디터 창 팔로워", () => {
  it("받은 테마를 CSS 변수에 입힌다", async () => {
    stop = startThemeFollower();
    await Promise.resolve();

    deliver(THEME_CHANGED_EVENT, getTheme(LIGHT));

    expect(getActiveTheme().id).toBe(LIGHT);
    expect(document.documentElement.style.getPropertyValue("--c-bg")).toBe(getTheme(LIGHT).tokens.bg);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("레지스트리에 없는 테마(편집 드래프트)도 그대로 입는다", async () => {
    stop = startThemeFollower();
    await Promise.resolve();
    const base = getTheme(DEFAULT_THEME_ID);
    const draft: Theme = {
      ...base,
      id: "custom-draft",
      tokens: { ...base.tokens, bg: "#123456" },
    };

    deliver(THEME_CHANGED_EVENT, draft);

    expect(document.documentElement.style.getPropertyValue("--c-bg")).toBe("#123456");
  });

  it("저장하지 않는다 — 드래프트 id가 다음 부팅으로 새면 안 된다", async () => {
    stop = startThemeFollower();
    await Promise.resolve();
    localStorage.clear();

    deliver(THEME_CHANGED_EVENT, getTheme(LIGHT));

    expect(localStorage.getItem("praxis-theme")).toBeNull();
  });
});
