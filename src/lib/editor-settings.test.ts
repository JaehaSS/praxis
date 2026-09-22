// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import {
  DEFAULT_EDITOR_SETTINGS,
  applyTreeMetrics,
  clampTreeFont,
  normalizeEditorSettings,
  treeMetrics,
} from "./editor-settings";

describe("treeMetrics", () => {
  it("기본 16px에서 기존 index.css 값을 그대로 재현한다", () => {
    // 이 기대값이 깨지면 설정을 건드리지 않은 사용자의 화면이 바뀐다는 뜻이다.
    expect(treeMetrics(16)).toEqual({
      fontSize: 16,
      lineHeight: 23,
      icon: 16,
      chevron: 15,
      indent: 14,
    });
  });

  it("치수가 한 벌로 함께 움직인다", () => {
    const small = treeMetrics(12);
    const large = treeMetrics(20);
    expect(small.lineHeight).toBeLessThan(large.lineHeight);
    expect(small.icon).toBeLessThan(large.icon);
    expect(small.indent).toBeLessThan(large.indent);
  });

  it("행 높이는 언제나 글자보다 크다 — 디센더가 잘리지 않게", () => {
    for (let size = 10; size <= 24; size += 1) {
      const m = treeMetrics(size);
      expect(m.lineHeight).toBeGreaterThan(m.fontSize);
    }
  });

  it("모든 치수가 정수 px다 — 소수면 행마다 반올림이 갈린다", () => {
    for (let size = 10; size <= 24; size += 1) {
      for (const v of Object.values(treeMetrics(size))) {
        expect(Number.isInteger(v)).toBe(true);
      }
    }
  });

  it("범위 밖 입력은 잘라서 받는다", () => {
    expect(treeMetrics(2).fontSize).toBe(10);
    expect(treeMetrics(99).fontSize).toBe(24);
  });
});

describe("clampTreeFont", () => {
  it("범위 안의 값은 그대로 둔다", () => {
    expect(clampTreeFont(13)).toBe(13);
  });

  it("소수는 내린다", () => {
    expect(clampTreeFont(13.7)).toBe(13);
  });

  it("숫자가 아니면 기본값으로 후퇴한다 — 빈 입력칸이 화면을 깨뜨리지 않게", () => {
    expect(clampTreeFont(Number.NaN)).toBe(DEFAULT_EDITOR_SETTINGS.tree_font_size);
    expect(clampTreeFont(Number.POSITIVE_INFINITY)).toBe(DEFAULT_EDITOR_SETTINGS.tree_font_size);
  });
});

describe("applyTreeMetrics", () => {
  it("다섯 변수를 모두 내린다 — 하나라도 빠지면 그 치수만 옛 값으로 남는다", () => {
    const root = document.createElement("div");
    applyTreeMetrics(root, 16);
    expect(root.style.getPropertyValue("--file-tree-font-size")).toBe("16px");
    expect(root.style.getPropertyValue("--file-tree-line-height")).toBe("23px");
    expect(root.style.getPropertyValue("--file-tree-icon")).toBe("16px");
    expect(root.style.getPropertyValue("--file-tree-chevron")).toBe("15px");
    expect(root.style.getPropertyValue("--file-tree-indent")).toBe("14px");
  });
});

describe("normalizeEditorSettings", () => {
  it("null이면 기본값", () => {
    expect(normalizeEditorSettings(null)).toEqual(DEFAULT_EDITOR_SETTINGS);
  });

  it("빠진 필드를 기본값으로 채운다 — 구버전 저장분이 와도 뜬다", () => {
    expect(normalizeEditorSettings({ tree_font_size: 12 })).toEqual({
      tree_font_size: 12,
      minimap: true,
      word_wrap: false,
      tab_size: 2,
    });
  });

  it("false를 기본값 true로 되돌리지 않는다 — 끈 것과 미설정은 다르다", () => {
    expect(normalizeEditorSettings({ minimap: false }).minimap).toBe(false);
  });

  it("탭 크기를 1..8로 자른다", () => {
    expect(normalizeEditorSettings({ tab_size: 0 }).tab_size).toBe(1);
    expect(normalizeEditorSettings({ tab_size: 40 }).tab_size).toBe(8);
  });
});
