import { describe, expect, it } from "vitest";
import { appendItems, eventToItem, isTurnComplete, parseConvoEvent, toItems } from "./convo";

describe("parseConvoEvent", () => {
  it("kind 태그 JSON만 대화 이벤트로 본다", () => {
    expect(parseConvoEvent('{"kind":"text","text":"hi"}')).toEqual({ kind: "text", text: "hi" });
  });

  it("터미널 조각과 깨진 JSON은 null", () => {
    // 터미널 모드 출력이 같은 테이블에 섞여 들어온다 — 여기서 걸러야 한다.
    expect(parseConvoEvent("\x1b[31mred")).toBeNull();
    expect(parseConvoEvent("{not json")).toBeNull();
    expect(parseConvoEvent('{"no":"kind"}')).toBeNull();
    expect(parseConvoEvent('"just a string"')).toBeNull();
    expect(parseConvoEvent("null")).toBeNull();
  });
});

describe("eventToItem", () => {
  it("주요 kind를 표시 아이템으로 바꾼다", () => {
    expect(eventToItem({ kind: "user", text: "해줘" })).toEqual({ role: "user", text: "해줘" });
    expect(eventToItem({ kind: "text", text: "네" })).toEqual({ role: "text", text: "네" });
    expect(eventToItem({ kind: "tool_use", name: "Read", summary: "a.ts" })).toEqual({
      role: "tool",
      name: "Read",
      summary: "a.ts",
    });
    expect(eventToItem({ kind: "tool_result", summary: "ok", is_error: false })).toEqual({
      role: "result",
      summary: "ok",
      error: false,
    });
    expect(eventToItem({ kind: "error", text: "터짐" })).toEqual({ role: "error", text: "터짐" });
  });

  it("누락 필드를 빈 값으로 채운다", () => {
    expect(eventToItem({ kind: "user" })).toEqual({ role: "user", text: "" });
    expect(eventToItem({ kind: "tool_use" })).toEqual({ role: "tool", name: "tool", summary: "" });
  });

  it("오류 결과를 구분한다", () => {
    expect(eventToItem({ kind: "tool_result", is_error: true })).toMatchObject({ error: true });
  });

  it("모르는 kind는 버린다", () => {
    // 정체불명의 원시 JSON을 폰에 띄우느니 없는 편이 낫다.
    expect(eventToItem({ kind: "user_expanded", text: "..." })).toBeNull();
    expect(eventToItem({ kind: "무언가" })).toBeNull();
  });
});

describe("toItems", () => {
  it("빈 텍스트 말풍선을 걸러낸다", () => {
    const items = toItems([
      { kind: "text", text: "  " },
      { kind: "text", text: "실제" },
      { kind: "unknown" },
    ]);
    expect(items).toEqual([{ role: "text", text: "실제" }]);
  });
});

describe("appendItems", () => {
  it("서버가 되돌려준 같은 user 메시지를 두 번 그리지 않는다", () => {
    const previous = toItems([{ kind: "user", text: "고쳐줘" }]);
    const incoming = toItems([
      { kind: "user", text: "고쳐줘" },
      { kind: "text", text: "알겠습니다" },
    ]);
    expect(appendItems(previous, incoming)).toEqual([
      { role: "user", text: "고쳐줘" },
      { role: "text", text: "알겠습니다" },
    ]);
  });

  it("내용이 다르면 둘 다 남긴다", () => {
    const previous = toItems([{ kind: "user", text: "A" }]);
    const incoming = toItems([{ kind: "user", text: "B" }]);
    expect(appendItems(previous, incoming)).toHaveLength(2);
  });

  it("빈 목록에도 안전하다", () => {
    expect(appendItems([], [])).toEqual([]);
    expect(appendItems([], [{ role: "text", text: "x" }])).toHaveLength(1);
  });
});

describe("isTurnComplete", () => {
  it("result가 오면 턴이 끝난 것으로 본다", () => {
    expect(isTurnComplete([{ kind: "text" }, { kind: "result" }])).toBe(true);
    expect(isTurnComplete([{ kind: "text" }])).toBe(false);
  });
});
