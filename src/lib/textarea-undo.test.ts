import { describe, expect, it } from "vitest";
import { createUndoStack, type TextSnapshot } from "./textarea-undo";

const snap = (value: string, caret = value.length): TextSnapshot => ({
  value,
  start: caret,
  end: caret,
});

/** 캐럿 끝에서 한 글자씩 타이핑한다 — 매 글자마다 stepMs가 흐른 것으로 본다. */
function type(
  stack: ReturnType<typeof createUndoStack>,
  from: string,
  text: string,
  at: number,
  stepMs = 50,
): { value: string; at: number } {
  let value = from;
  let now = at;
  for (const ch of text) {
    const next = value + ch;
    stack.record(snap(value), snap(next), now);
    value = next;
    now += stepMs;
  }
  return { value, at: now };
}

describe("textarea 되돌리기 스택", () => {
  it("연속 타이핑을 한 묶음으로 되돌린다", () => {
    const stack = createUndoStack();
    const typed = type(stack, "", "hello", 1000);

    expect(stack.undo(snap(typed.value))?.value).toBe("");
    expect(stack.canUndo()).toBe(false);
  });

  it("묶기 간격을 넘기면 새 묶음이 된다", () => {
    const stack = createUndoStack({ coalesceMs: 800 });
    const first = type(stack, "", "ab", 1000);
    const second = type(stack, first.value, "cd", first.at + 900);

    expect(stack.undo(snap(second.value))?.value).toBe("ab");
    expect(stack.undo(snap("ab"))?.value).toBe("");
  });

  it("공백 뒤에 이어 친 글자는 새 묶음이다", () => {
    const stack = createUndoStack();
    const typed = type(stack, "", "ab cd", 1000);

    expect(stack.undo(snap(typed.value))?.value).toBe("ab ");
    expect(stack.undo(snap("ab "))?.value).toBe("");
  });

  it("붙여넣기는 타이핑과 묶이지 않고 별도 단계로 남는다", () => {
    const stack = createUndoStack();
    const typed = type(stack, "", "ab", 1000);
    stack.record(snap(typed.value), snap("abPASTED"), typed.at);
    const after = type(stack, "abPASTED", "z", typed.at + 50);

    expect(stack.undo(snap(after.value))?.value).toBe("abPASTED");
    expect(stack.undo(snap("abPASTED"))?.value).toBe("ab");
    expect(stack.undo(snap("ab"))?.value).toBe("");
  });

  it("한 글자 삭제도 한 묶음으로 되돌린다", () => {
    const stack = createUndoStack();
    stack.record(snap("abc"), snap("ab"), 1000);
    stack.record(snap("ab"), snap("a"), 1050);

    expect(stack.undo(snap("a"))?.value).toBe("abc");
  });

  it("되돌린 뒤 다시하기로 복귀하고, 새 편집은 다시하기를 비운다", () => {
    const stack = createUndoStack();
    const typed = type(stack, "", "hi", 1000);

    const undone = stack.undo(snap(typed.value));
    expect(undone?.value).toBe("");
    expect(stack.canRedo()).toBe(true);
    expect(stack.redo(snap(""))?.value).toBe("hi");

    stack.undo(snap("hi"));
    type(stack, "", "x", 5000);
    expect(stack.canRedo()).toBe(false);
  });

  it("되돌릴 것이 없으면 null을 준다", () => {
    const stack = createUndoStack();
    expect(stack.undo(snap("a"))).toBeNull();
    expect(stack.redo(snap("a"))).toBeNull();
  });

  it("상한을 넘으면 가장 오래된 것부터 버린다", () => {
    const stack = createUndoStack({ limit: 2, coalesceMs: 0 });
    stack.record(snap(""), snap("a"), 1000);
    stack.record(snap("a"), snap("ab"), 2000);
    stack.record(snap("ab"), snap("abc"), 3000);

    expect(stack.undo(snap("abc"))?.value).toBe("ab");
    expect(stack.undo(snap("ab"))?.value).toBe("a");
    expect(stack.canUndo()).toBe(false);
  });

  it("kind 힌트가 분류를 덮는다 — 같은 길이 치환도 앞 타이핑과 묶인다", () => {
    const stack = createUndoStack();
    const typed = type(stack, "", "ab", 1000);
    // 길이가 같은 치환은 분류상 other지만, IME 조합처럼 호출자가 아는 경우 insert로 넘긴다.
    stack.record(snap(typed.value), snap("aX"), typed.at, "insert");

    expect(stack.undo(snap("aX"))?.value).toBe("");
  });

  it("선택 영역을 지운 치환은 별도 단계로 남는다", () => {
    const stack = createUndoStack();
    const typed = type(stack, "", "ab", 1000);
    stack.record({ value: typed.value, start: 0, end: 2 }, snap("c"), typed.at);

    expect(stack.undo(snap("c"))?.value).toBe("ab");
    expect(stack.undo(snap("ab"))?.value).toBe("");
  });

  it("상한에 닿은 스택에서 다시하기를 해도 되돌리기가 어긋나지 않는다", () => {
    const stack = createUndoStack({ limit: 2, coalesceMs: 0 });
    stack.record(snap(""), snap("a"), 1000);
    stack.record(snap("a"), snap("ab"), 2000);
    stack.record(snap("ab"), snap("abc"), 3000);
    stack.undo(snap("abc"));
    stack.redo(snap("ab"));

    expect(stack.undo(snap("abc"))?.value).toBe("ab");
    expect(stack.undo(snap("ab"))?.value).toBe("a");
    expect(stack.canUndo()).toBe(false);
  });

  it("되돌린 자리의 선택 영역까지 복원한다", () => {
    const stack = createUndoStack();
    stack.record({ value: "abc", start: 1, end: 3 }, snap("aX", 2), 1000);

    expect(stack.undo(snap("aX", 2))).toEqual({ value: "abc", start: 1, end: 3 });
  });
});
