import { describe, expect, it, vi } from "vitest";
import { requestComposerFocus, subscribeComposerFocus } from "./composer-focus";

describe("composer focus", () => {
  it("같은 작업의 구독자에게만 요청을 전달한다", () => {
    const mine = vi.fn();
    const other = vi.fn();
    const unsubMine = subscribeComposerFocus(7, mine);
    const unsubOther = subscribeComposerFocus(8, other);

    requestComposerFocus(7);

    expect(mine).toHaveBeenCalledTimes(1);
    expect(other).not.toHaveBeenCalled();
    unsubMine();
    unsubOther();
  });

  it("구독 해제 후에는 호출되지 않는다", () => {
    const listener = vi.fn();
    subscribeComposerFocus(7, listener)();
    requestComposerFocus(7);
    expect(listener).not.toHaveBeenCalled();
  });

  it("구독자가 없어도 요청은 조용히 무시된다", () => {
    expect(() => requestComposerFocus(999)).not.toThrow();
  });
});
