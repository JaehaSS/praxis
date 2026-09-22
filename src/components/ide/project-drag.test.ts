import { describe, expect, it } from "vitest";

import {
  acceptsProjectDrop,
  draggedProjectKind,
  PROJECT_DRAG_MIME,
  PROJECT_GROUP_DRAG_MIME,
} from "./project-drag";

describe("draggedProjectKind", () => {
  it("우리 MIME을 알아본다", () => {
    expect(draggedProjectKind([PROJECT_DRAG_MIME])).toBe("project");
    expect(draggedProjectKind([PROJECT_GROUP_DRAG_MIME])).toBe("group");
  });

  // 파일·탭 드래그가 사이드바 위를 지나도 강조되면 안 된다.
  it("모르는 것과 빈 것은 null이다", () => {
    expect(draggedProjectKind(["text/plain", "Files"])).toBeNull();
    expect(draggedProjectKind([])).toBeNull();
    expect(draggedProjectKind(undefined)).toBeNull();
  });
});

describe("acceptsProjectDrop", () => {
  it("다른 그룹으로 가는 프로젝트만 받는다", () => {
    expect(acceptsProjectDrop("project", null, "g1")).toBe(true);
    expect(acceptsProjectDrop("project", "g1", null)).toBe(true);
  });

  // 강조는 "놓으면 무언가 달라진다"는 약속이다.
  it("자기 자리와 그룹 드래그와 모르는 드래그는 받지 않는다", () => {
    expect(acceptsProjectDrop("project", "g1", "g1")).toBe(false);
    expect(acceptsProjectDrop("project", null, null)).toBe(false);
    expect(acceptsProjectDrop("group", null, "g1")).toBe(false);
    expect(acceptsProjectDrop(null, null, "g1")).toBe(false);
  });
});
