import { describe, expect, it } from "vitest";
import {
  alpha,
  contrastRatio,
  darken,
  ensureContrast,
  hexAlpha,
  lighten,
  mix,
  parseHex,
  toHex,
} from "./color";

describe("hex 파싱", () => {
  it("3자리 축약형을 6자리로 편다", () => {
    expect(parseHex("#abc")).toEqual(parseHex("#aabbcc"));
  });

  it("왕복해도 값이 보존된다", () => {
    expect(toHex(parseHex("#14b8a6"))).toBe("#14b8a6");
  });

  it("망가진 입력은 검정으로 떨어진다 — 테마 정의는 상수라 런타임에 올 값이 아니다", () => {
    expect(toHex(parseHex("nope"))).toBe("#000000");
  });
});

describe("혼합·투명도", () => {
  it("t=0과 t=1은 양 끝값 그대로다", () => {
    expect(mix("#000000", "#ffffff", 0)).toBe("#000000");
    expect(mix("#000000", "#ffffff", 1)).toBe("#ffffff");
  });

  it("중간값은 절반이다", () => {
    expect(mix("#000000", "#ffffff", 0.5)).toBe("#808080");
  });

  it("rgba()와 8자리 hex는 같은 색을 가리킨다", () => {
    expect(alpha("#14b8a6", 0.2)).toBe("rgba(20, 184, 166, 0.2)");
    expect(hexAlpha("#14b8a6", 0.2)).toBe("#14b8a633");
  });
});

describe("명도 조정", () => {
  it("색상과 채도는 두고 명도만 움직인다", () => {
    // teal은 밝게 해도 teal이어야 한다 — RGB 가산이면 흰색으로 바랜다.
    const brighter = lighten("#14b8a6", 0.1);
    expect(contrastRatio(brighter, "#000000")).toBeGreaterThan(
      contrastRatio("#14b8a6", "#000000"),
    );
    expect(parseHex(brighter).g).toBeGreaterThan(parseHex(brighter).r);
  });

  it("어둡게 하면 대비가 반대로 움직인다", () => {
    expect(contrastRatio(darken("#14b8a6", 0.1), "#000000")).toBeLessThan(
      contrastRatio("#14b8a6", "#000000"),
    );
  });

  it("경계를 넘어가지 않는다", () => {
    expect(lighten("#ffffff", 0.5)).toBe("#ffffff");
    expect(darken("#000000", 0.5)).toBe("#000000");
  });
});

describe("명암비", () => {
  it("흑백은 21:1", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
  });

  it("같은 색은 1:1", () => {
    expect(contrastRatio("#3b4252", "#3b4252")).toBeCloseTo(1, 5);
  });
});

describe("대비 보정", () => {
  it("이미 충분하면 원본을 그대로 돌려준다", () => {
    expect(ensureContrast("#f4f4f5", "#0d0d0d", 7)).toBe("#f4f4f5");
  });

  it("모자라면 목표를 채운다 — Nord nord3는 원본이 저대비다", () => {
    const fixed = ensureContrast("#4c566a", "#2e3440", 3.5);
    expect(contrastRatio("#4c566a", "#2e3440")).toBeLessThan(3.5);
    expect(contrastRatio(fixed, "#2e3440")).toBeGreaterThanOrEqual(3.5);
  });

  it("어두운 배경에서는 밝아지고, 밝은 배경에서는 어두워진다", () => {
    expect(parseHex(ensureContrast("#808080", "#000000", 7)).r).toBeGreaterThan(0x80);
    expect(parseHex(ensureContrast("#808080", "#ffffff", 7)).r).toBeLessThan(0x80);
  });

  it("극단까지 가도 목표에 못 미치면 그 극단값을 준다", () => {
    // 흰 배경에서 21:1을 넘길 수는 없다 — 검정이 한계다.
    expect(ensureContrast("#808080", "#ffffff", 21)).toBe("#000000");
  });
});
