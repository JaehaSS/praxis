import { describe, expect, it } from "vitest";
import {
  readEditorCaptureTarget,
  readEditorSelection,
  registerEditorCaptureTarget,
  type EditorCaptureTarget,
} from "./editor-capture-target";

const target: EditorCaptureTarget = {
  bounds: { x: 1, y: 2, width: 300, height: 200 },
  file_path: "src/App.tsx",
  selection_text: "selected",
  selection_start_line: 10,
  selection_end_line: 12,
};

describe("editor capture target", () => {
  it("task id별 현재 에디터 캡처 정보를 읽고 등록 해제한다", () => {
    const unregister = registerEditorCaptureTarget(7, () => target);
    expect(readEditorCaptureTarget(7)).toEqual(target);
    expect(readEditorCaptureTarget(8)).toBeNull();
    unregister();
    expect(readEditorCaptureTarget(7)).toBeNull();
  });

  it("bounds가 없으면 스크린샷 캡처는 불가하지만 선택 첨부는 가능하다", () => {
    // 에디터가 화면 밖이라 촬영 영역을 계산하지 못한 상태 — ⌘L(텍스트)은 계속 동작해야 한다.
    const unregister = registerEditorCaptureTarget(7, () => ({ ...target, bounds: null }));
    expect(readEditorCaptureTarget(7)).toBeNull();
    expect(readEditorSelection(7)).toEqual({
      file_path: "src/App.tsx",
      selection_text: "selected",
      selection_start_line: 10,
      selection_end_line: 12,
    });
    unregister();
  });

  it("등록된 에디터가 없으면 선택도 null이다", () => {
    expect(readEditorSelection(99)).toBeNull();
  });

  it("이전 등록의 cleanup이 최신 reader를 지우지 않는다", () => {
    const unregisterOld = registerEditorCaptureTarget(7, () => target);
    const latest = { ...target, file_path: "src/new.ts" };
    const unregisterLatest = registerEditorCaptureTarget(7, () => latest);
    unregisterOld();
    expect(readEditorCaptureTarget(7)?.file_path).toBe("src/new.ts");
    unregisterLatest();
  });
});
