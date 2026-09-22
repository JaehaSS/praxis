import { describe, it, expect } from "vitest";
import { b64ToBytes } from "./bytes";

describe("b64ToBytes", () => {
  it("decodes base64 to bytes", () => {
    // "hi" → base64 "aGk="
    expect(Array.from(b64ToBytes("aGk="))).toEqual([104, 105]);
  });

  it("preserves arbitrary bytes (ANSI ESC sequence)", () => {
    const b64 = btoa(String.fromCharCode(0x1b, 0x5b, 0x41)); // ESC [ A
    expect(Array.from(b64ToBytes(b64))).toEqual([0x1b, 0x5b, 0x41]);
  });
});
