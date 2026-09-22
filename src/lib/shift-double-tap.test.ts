import { describe, expect, it } from "vitest";
import { onKeyDown, onKeyUp, SHIFT_DOUBLE_TAP_MS, type ShiftTapState } from "./shift-double-tap";

const S = { key: "Shift" };
const fresh = (): ShiftTapState => ({ lastUp: 0 });

describe("Shift 더블탭", () => {
  it("창 안에 두 번이면 발동한다", () => {
    const a = onKeyUp(fresh(), S, 1000);
    expect(a.fire).toBe(false);
    expect(onKeyUp(a.next, S, 1000 + SHIFT_DOUBLE_TAP_MS - 1).fire).toBe(true);
  });

  it("창을 넘기면 발동하지 않는다", () => {
    const a = onKeyUp(fresh(), S, 1000);
    expect(onKeyUp(a.next, S, 1000 + SHIFT_DOUBLE_TAP_MS + 1).fire).toBe(false);
  });

  it("사이에 다른 키가 끼면 취소된다", () => {
    // Shift+A 입력 중의 Shift 두 번은 의도가 아니다.
    const a = onKeyUp(fresh(), S, 1000);
    const cancelled = onKeyDown(a.next, { key: "a" });
    expect(onKeyUp(cancelled, S, 1010).fire).toBe(false);
  });

  it("다른 수식자와 함께면 잡아먹지 않는다", () => {
    const a = onKeyUp(fresh(), S, 1000);
    expect(onKeyUp(a.next, { key: "Shift", metaKey: true }, 1010).fire).toBe(false);
  });

  it("IME 조합 중이면 무시한다", () => {
    const a = onKeyUp(fresh(), S, 1000);
    expect(onKeyUp(a.next, { key: "Shift", isComposing: true }, 1010).fire).toBe(false);
  });

  it("Shift가 아닌 keyup은 상태를 건드리지 않는다", () => {
    const a = onKeyUp(fresh(), S, 1000);
    const b = onKeyUp(a.next, { key: "a" }, 1010);
    expect(b.next).toBe(a.next);
    expect(onKeyUp(b.next, S, 1020).fire).toBe(true);
  });

  it("세 번 누르면 두 번째에 한 번만 발동한다", () => {
    const a = onKeyUp(fresh(), S, 1000);
    const b = onKeyUp(a.next, S, 1100);
    expect(b.fire).toBe(true);
    expect(onKeyUp(b.next, S, 1150).fire).toBe(false);
  });
});
