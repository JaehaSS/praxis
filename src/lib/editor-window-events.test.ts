import { describe, expect, it } from "vitest";
import { isEditorEntry } from "./editor-window-events";

describe("isEditorEntry", () => {
  it("editor 창 진입만 참", () => {
    expect(isEditorEntry("?window=editor")).toBe(true);
    expect(isEditorEntry("?window=other")).toBe(false);
    expect(isEditorEntry("")).toBe(false);
  });

  it("부분 일치로 오인하지 않는다", () => {
    // 접두사만 같은 값이 통과하면 다른 창이 에디터로 열려 앱이 통째로 잘못 뜬다.
    expect(isEditorEntry("?window=editor-preview")).toBe(false);
    expect(isEditorEntry("?window=preview-editor")).toBe(false);
  });

  it("다른 쿼리 파라미터가 있어도 window만 본다", () => {
    expect(isEditorEntry("?task=42&window=editor")).toBe(true);
    expect(isEditorEntry("?editor=1")).toBe(false);
  });
});
