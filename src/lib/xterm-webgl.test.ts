import { describe, expect, it, vi } from "vitest";
import type { Terminal } from "@xterm/xterm";
import type { WebglAddon } from "@xterm/addon-webgl";
import { disposeTerminal, disposeWebgl } from "./xterm-webgl";

describe("disposeWebgl", () => {
  it("addon dispose가 throw해도 예외를 전파하지 않는다", () => {
    const addon = {
      dispose: vi.fn(() => {
        throw new TypeError("Cannot read properties of undefined (reading '_isDisposed')");
      }),
    } as unknown as WebglAddon;

    expect(() => disposeWebgl(addon)).not.toThrow();
    expect(addon.dispose).toHaveBeenCalledTimes(1);
  });

  it("addon이 없으면 아무것도 하지 않는다", () => {
    expect(() => disposeWebgl(null)).not.toThrow();
  });
});

describe("disposeTerminal", () => {
  it("term dispose가 throw해도 예외를 전파하지 않는다", () => {
    const term = {
      dispose: vi.fn(() => {
        throw new TypeError("addon dispose failed");
      }),
    } as unknown as Terminal;

    expect(() => disposeTerminal(term)).not.toThrow();
    expect(term.dispose).toHaveBeenCalledTimes(1);
  });
});
