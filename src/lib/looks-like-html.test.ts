import { describe, expect, it } from "vitest";
import { looksLikeHtml } from "./looks-like-html";

describe("looksLikeHtml — 참", () => {
  it("표 조각 전체", () => {
    expect(looksLikeHtml("<table><tr><td>셀</td></tr></table>")).toBe(true);
  });

  it("완전한 문서", () => {
    expect(looksLikeHtml("<!DOCTYPE html><html><body><p>본문</p></body></html>")).toBe(true);
  });

  it("앞뒤 공백·줄바꿈은 무시한다", () => {
    expect(looksLikeHtml("\n\n  <table><tr><td>셀</td></tr></table>  \n")).toBe(true);
  });

  it("선두 태그에 속성이 붙어도 본다", () => {
    expect(looksLikeHtml('<table class="x" id="y"><tr><td>셀</td></tr></table>')).toBe(true);
  });

  it("doctype 없이 <html>로 시작해도 문서다", () => {
    expect(looksLikeHtml("<html><body>x</body></html>")).toBe(true);
  });

  it("선두 생성기 주석은 건너뛴다", () => {
    expect(looksLikeHtml("<!-- gen -->\n<table><tr><td>셀</td></tr></table>")).toBe(true);
  });

  it("같은 태그가 중첩돼도 마지막에 닫히면 한 덩어리다", () => {
    expect(looksLikeHtml("<div><div>안쪽</div></div>")).toBe(true);
  });
});

describe("looksLikeHtml — 거짓", () => {
  it("보통 마크다운 본문", () => {
    expect(looksLikeHtml("# 제목\n\n본문이다.\n\n| a | b |\n|---|---|\n| 1 | 2 |")).toBe(false);
  });

  it("HTML을 설명하는 문장 — 태그로 시작해도 문서가 아니다", () => {
    expect(looksLikeHtml("<table> 태그는 뭔가요?")).toBe(false);
  });

  it("인라인 태그로 시작하는 것은 렌더 대상이 아니다", () => {
    expect(looksLikeHtml("<span>x</span>")).toBe(false);
  });

  it("열린 채 닫히지 않은 태그", () => {
    expect(looksLikeHtml("<div>내용만 있고 안 닫혔다")).toBe(false);
  });

  it("마크다운에 HTML이 섞인 혼합 문서", () => {
    expect(looksLikeHtml("# 제목\n\n<table><tr><td>셀</td></tr></table>")).toBe(false);
  });

  it("블록으로 시작하지만 뒤에 다른 내용이 이어지는 경우", () => {
    expect(looksLikeHtml("<table><tr><td>셀</td></tr></table> 그리고 <b>덧말</b>")).toBe(false);
  });

  it("빈 문자열", () => {
    expect(looksLikeHtml("   ")).toBe(false);
  });

  it("배지 div로 시작해 푸터 div로 끝나는 README — 가운데가 마크다운이다", () => {
    const readme = [
      '<div align="center">',
      '  <img src="logo.png">',
      "</div>",
      "",
      "# 제목",
      "본문…",
      "",
      '<div align="center"><sub>© 2026</sub></div>',
    ].join("\n");

    expect(looksLikeHtml(readme)).toBe(false);
  });

  it("스트리밍 중의 반쪽 문서", () => {
    expect(looksLikeHtml("<!DOCTYPE html><html><head>")).toBe(false);
  });
});
