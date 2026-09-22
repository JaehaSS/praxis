// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from "vitest";
import { previewTabsEnabled, setPreviewTabsEnabled } from "./preview-tabs";

beforeEach(() => localStorage.clear());

describe("토글", () => {
  it("저장된 값이 없으면 켬이다", () => {
    expect(previewTabsEnabled()).toBe(true);
  });

  it("끈 뒤에는 꺼진 채로 남는다", () => {
    setPreviewTabsEnabled(false);
    expect(previewTabsEnabled()).toBe(false);
    setPreviewTabsEnabled(true);
    expect(previewTabsEnabled()).toBe(true);
  });
});
