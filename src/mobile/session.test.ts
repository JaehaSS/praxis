import { describe, expect, it } from "vitest";
import { readPairingCode } from "./session";

const VALID = "a".repeat(64);

describe("readPairingCode", () => {
  it("pair 프래그먼트에서 코드를 꺼낸다", () => {
    expect(readPairingCode(`#pair=${VALID}`)).toBe(VALID);
    expect(readPairingCode(`pair=${VALID}`)).toBe(VALID);
    expect(readPairingCode(`#x=1&pair=${VALID}`)).toBe(VALID);
  });

  it("형식이 다른 값은 무시한다", () => {
    // 길이·문자셋이 어긋나면 서버에 보낼 이유가 없다.
    expect(readPairingCode("#pair=short")).toBeNull();
    expect(readPairingCode(`#pair=${"A".repeat(64)}`)).toBeNull();
    expect(readPairingCode(`#pair=${"g".repeat(64)}`)).toBeNull();
    expect(readPairingCode(`#pair=${VALID}extra`)).toBeNull();
  });

  it("코드가 없으면 null", () => {
    expect(readPairingCode("")).toBeNull();
    expect(readPairingCode("#")).toBeNull();
    expect(readPairingCode("#other=1")).toBeNull();
    expect(readPairingCode("#pair=")).toBeNull();
  });
});
