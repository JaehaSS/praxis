import { describe, expect, it } from "vitest";
import { isConversationNearBottom } from "./conversationScroll";

describe("isConversationNearBottom", () => {
  it("새 출력 전 이미 하단을 보고 있으면 출력을 계속 따라간다", () => {
    expect(
      isConversationNearBottom({ scrollHeight: 1000, scrollTop: 500, clientHeight: 500 }),
    ).toBe(true);
  });

  it("응답 이력을 보기 위해 위로 이동했으면 새 출력을 따라가지 않는다", () => {
    expect(
      isConversationNearBottom({ scrollHeight: 1000, scrollTop: 300, clientHeight: 500 }),
    ).toBe(false);
  });

  it("하단 근처의 작은 오차는 자동 스크롤 상태로 유지한다", () => {
    expect(
      isConversationNearBottom({ scrollHeight: 1000, scrollTop: 455, clientHeight: 500 }),
    ).toBe(true);
  });
});
