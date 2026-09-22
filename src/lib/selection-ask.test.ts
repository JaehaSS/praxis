import { describe, expect, it } from "vitest";
import { bubblePlacement, buildAskPayload, shouldShowBubble } from "./selection-ask";

describe("shouldShowBubble", () => {
  it("실제 코드를 선택하면 뜬다", () => {
    expect(shouldShowBubble("const x = 1;")).toBe(true);
    expect(shouldShowBubble("}")).toBe(true);
  });

  it("빈 선택이면 뜨지 않는다", () => {
    expect(shouldShowBubble("")).toBe(false);
  });

  it("공백만 선택하면 뜨지 않는다", () => {
    // 드래그가 빗나가 들여쓰기만 잡힌 경우다. 물어볼 것이 없다.
    expect(shouldShowBubble("   \n\t ")).toBe(false);
  });
});

describe("bubblePlacement", () => {
  it("아래 여백이 넉넉하면 아래에 붙인다", () => {
    expect(bubblePlacement(200, 120)).toBe("below");
    expect(bubblePlacement(120, 120)).toBe("below");
  });

  it("아래가 좁으면 위로 뒤집는다", () => {
    // 파일 끝을 선택했을 때 버블이 화면 밖으로 나가는 것을 막는다.
    expect(bubblePlacement(40, 120)).toBe("above");
  });
});

describe("buildAskPayload", () => {
  const base = {
    taskId: 42,
    filePath: "src/a.ts",
    selectionText: "const x = 1;",
    startLine: 3,
    endLine: 3,
  };

  it("질문과 선택을 함께 싣는다", () => {
    expect(buildAskPayload({ ...base, question: "이게 왜 필요해?" })).toEqual({
      task_id: 42,
      file_path: "src/a.ts",
      start_line: 3,
      end_line: 3,
      selection_text: "const x = 1;",
      question: "이게 왜 필요해?",
    });
  });

  it("질문 앞뒤 공백은 다듬는다", () => {
    expect(buildAskPayload({ ...base, question: "  왜?  " })?.question).toBe("왜?");
  });

  it("질문이 비면 보낼 것이 없다", () => {
    expect(buildAskPayload({ ...base, question: "   " })).toBeNull();
  });

  it("선택이 비면 보낼 것이 없다", () => {
    expect(buildAskPayload({ ...base, selectionText: "  ", question: "왜?" })).toBeNull();
  });
});
