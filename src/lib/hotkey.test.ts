import { describe, expect, it } from "vitest";
import { captureShortcut, formatShortcut, normalizeShortcut } from "./hotkey";

/** 캡처 입력이 실제로 받는 모양 — `key` 는 일부러 넣지 않는다(모듈이 봐서는 안 되는 값). */
const press = (
  code: string,
  mods: Partial<{ ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }> = {},
) => ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });

describe("captureShortcut", () => {
  it("macOS 에서 Option+C 를 Alt+C 로 잡는다", () => {
    // 이 조합의 event.key 는 "ç" 다. code 를 보지 않으면 저장값이 파서에 거부된다.
    expect(captureShortcut(press("KeyC", { altKey: true }))).toEqual({ kind: "ok", spec: "Alt+C" });
  });

  it("modifier 를 누르고 있는 중간 상태는 확정하지 않는다", () => {
    expect(captureShortcut(press("AltLeft", { altKey: true }))).toEqual({ kind: "pending" });
    expect(captureShortcut(press("ShiftRight", { shiftKey: true }))).toEqual({ kind: "pending" });
    expect(captureShortcut(press("MetaLeft", { metaKey: true }))).toEqual({ kind: "pending" });
  });

  it("조합 없는 낱개 키는 거부한다 — 전역에서 그 키를 빼앗는다", () => {
    expect(captureShortcut(press("KeyC"))).toEqual({ kind: "bare" });
    expect(captureShortcut(press("Space"))).toEqual({ kind: "bare" });
  });

  it("기능키는 낱개로도 받는다", () => {
    expect(captureShortcut(press("F5"))).toEqual({ kind: "ok", spec: "F5" });
    expect(captureShortcut(press("F13"))).toEqual({ kind: "ok", spec: "F13" });
  });

  it("파서가 모르는 키는 지원하지 않는다고 알린다", () => {
    expect(captureShortcut(press("Lang1", { altKey: true }))).toEqual({ kind: "unsupported" });
    expect(captureShortcut(press("IntlBackslash", { altKey: true }))).toEqual({ kind: "unsupported" });
  });

  it("modifier 순서를 ⌃⌥⇧⌘ 로 고정한다", () => {
    const all = press("KeyK", { metaKey: true, shiftKey: true, altKey: true, ctrlKey: true });
    expect(captureShortcut(all)).toEqual({ kind: "ok", spec: "Ctrl+Alt+Shift+Cmd+K" });
  });

  it("숫자·화살표·기호 키를 파서가 아는 토큰으로 낸다", () => {
    expect(captureShortcut(press("Digit1", { altKey: true }))).toEqual({ kind: "ok", spec: "Alt+1" });
    expect(captureShortcut(press("ArrowUp", { altKey: true }))).toEqual({ kind: "ok", spec: "Alt+ArrowUp" });
    expect(captureShortcut(press("BracketLeft", { ctrlKey: true }))).toEqual({
      kind: "ok",
      spec: "Ctrl+BracketLeft",
    });
  });
});

describe("normalizeShortcut", () => {
  it("표기가 달라도 같은 조합이면 같은 문자열이 된다", () => {
    expect(normalizeShortcut("Shift+Alt+C", true)).toBe("Alt+Shift+C");
    expect(normalizeShortcut("Option+Shift+KeyC", true)).toBe("Alt+Shift+C");
    expect(normalizeShortcut("alt+shift+c", true)).toBe("Alt+Shift+C");
  });

  it("CmdOrCtrl 은 플랫폼에 따라 갈린다", () => {
    expect(normalizeShortcut("CmdOrCtrl+Shift+A", true)).toBe("Shift+Cmd+A");
    expect(normalizeShortcut("CmdOrCtrl+Shift+A", false)).toBe("Ctrl+Shift+A");
  });

  it("주 키가 없거나 둘이면 해석하지 않는다", () => {
    expect(normalizeShortcut("Alt+Shift", true)).toBeNull();
    expect(normalizeShortcut("Alt+C+D", true)).toBeNull();
    expect(normalizeShortcut("", true)).toBeNull();
  });
});

describe("formatShortcut", () => {
  it("macOS 는 기호로 붙여 쓴다 — Alt 라는 이름을 보이지 않는다", () => {
    expect(formatShortcut("Alt+C", true)).toBe("⌥C");
    expect(formatShortcut("Ctrl+Alt+Shift+Cmd+K", true)).toBe("⌃⌥⇧⌘K");
    expect(formatShortcut("CmdOrCtrl+Shift+A", true)).toBe("⇧⌘A");
  });

  it("그 외 플랫폼은 이름을 + 로 잇는다", () => {
    expect(formatShortcut("Ctrl+Alt+C", false)).toBe("Ctrl+Alt+C");
    expect(formatShortcut("CmdOrCtrl+Shift+A", false)).toBe("Ctrl+Shift+A");
  });

  it("화살표와 Escape 는 짧은 기호로 보인다", () => {
    expect(formatShortcut("Alt+ArrowUp", true)).toBe("⌥↑");
    expect(formatShortcut("Ctrl+Escape", false)).toBe("Ctrl+Esc");
  });

  it("해석할 수 없는 값은 감추지 않고 그대로 보여 준다", () => {
    expect(formatShortcut("", true)).toBe("");
    expect(formatShortcut("Alt+C+D", true)).toBe("Alt+C+D");
  });
});
