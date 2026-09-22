import { describe, it, expect } from "vitest";
import { quoteFamily, codeFontStack, uiFontStack } from "./fonts";

const CODE_CHAIN =
  "'JetBrains Mono', 'Fira Code', 'Cascadia Code', 'Cascadia Mono', Consolas, ui-monospace, monospace";
const UI_CHAIN =
  "'Inter Variable', Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI Variable Text', 'Segoe UI', 'Pretendard Variable', Pretendard, 'Apple SD Gothic Neo', 'Malgun Gothic', sans-serif";

describe("quoteFamily", () => {
  it("따옴표를 제거한다", () => {
    expect(quoteFamily("Je'tendard")).toBe("'Jetendard'");
  });

  it("백슬래시를 제거한다", () => {
    expect(quoteFamily("Jet\\endard")).toBe("'Jetendard'");
  });

  it("양끝 공백을 제거한다", () => {
    expect(quoteFamily("  Jetendard  ")).toBe("'Jetendard'");
  });
});

describe("codeFontStack", () => {
  it("빈 문자열이면 기본 체인 그대로", () => {
    expect(codeFontStack("")).toBe(CODE_CHAIN);
  });

  it("undefined면 기본 체인 그대로", () => {
    expect(codeFontStack(undefined)).toBe(CODE_CHAIN);
  });

  it("지정 시 체인 앞에 prepend", () => {
    expect(codeFontStack("Jetendard")).toBe(`'Jetendard', ${CODE_CHAIN}`);
  });

  it("Windows 기본 탑재 고정폭이 generic 폴백보다 앞에 온다", () => {
    const chain = codeFontStack("");
    expect(chain.indexOf("Consolas")).toBeLessThan(chain.indexOf("monospace"));
  });
});

describe("uiFontStack", () => {
  it("빈 문자열이면 기본 체인 그대로", () => {
    expect(uiFontStack("")).toBe(UI_CHAIN);
  });

  it("undefined면 기본 체인 그대로", () => {
    expect(uiFontStack(undefined)).toBe(UI_CHAIN);
  });

  it("지정 시 체인 앞에 prepend", () => {
    expect(uiFontStack("Jetendard")).toBe(`'Jetendard', ${UI_CHAIN}`);
  });

  it("각 OS의 시스템 UI 서체를 generic sans-serif보다 앞에 둔다", () => {
    const chain = uiFontStack("");
    for (const family of ["-apple-system", "'Segoe UI'", "'Malgun Gothic'"]) {
      expect(chain.indexOf(family)).toBeGreaterThan(-1);
      expect(chain.indexOf(family)).toBeLessThan(chain.indexOf("sans-serif"));
    }
  });
});
