/**
 * 폰트명 이스케이프 + 폴백 체인 조립 (순수 함수).
 * 선택 폰트는 항상 기존 체인 앞에 prepend — 미설치/오타 폰트는 브라우저가 조용히 폴백.
 * 설계 정본: docs/designs/0007.2026-07-09-font-settings-design.md §2
 */

/** CSS/JS 옵션 문자열 파손 방지 — 따옴표·백슬래시 제거 후 양끝 공백 트림, 홑따옴표로 감쌈. */
export const quoteFamily = (f: string) => `'${f.replace(/[\\']/g, "").trim()}'`;

/** 코드 폰트 스택. 미지정(빈 문자열/undefined) 시 기본 체인 그대로.
 *
 * Windows에는 JetBrains Mono/Fira Code가 없는 게 보통이라 `ui-monospace`로 떨어지는데,
 * 그러면 Courier 계열이 잡혀 코드가 눈에 띄게 조악해진다 — Windows 기본 탑재인
 * Cascadia/Consolas를 그 앞에 둔다. */
export const codeFontStack = (f?: string) =>
  [
    f && quoteFamily(f),
    "'JetBrains Mono'",
    "'Fira Code'",
    "'Cascadia Code'",
    "'Cascadia Mono'",
    "Consolas",
    "ui-monospace",
    "monospace",
  ]
    .filter(Boolean)
    .join(", ");

/** UI 폰트 스택. 미지정(빈 문자열/undefined) 시 기본 체인 그대로.
 *
 * 폴백은 문자 단위로 적용되므로 라틴용과 한글용을 함께 둔다. Windows에서 Inter가 없으면
 * `-apple-system`/`BlinkMacSystemFont`도 잡히지 않아 곧장 `sans-serif`(Arial)로 떨어졌고,
 * 한글은 맑은 고딕이 붙어 자간·굵기가 어긋나 보였다 — 각 OS의 시스템 UI 서체를 명시한다. */
export const uiFontStack = (f?: string) =>
  [
    f && quoteFamily(f),
    "'Inter Variable'",
    "Inter",
    "-apple-system",
    "BlinkMacSystemFont",
    "'Segoe UI Variable Text'",
    "'Segoe UI'",
    "'Pretendard Variable'",
    "Pretendard",
    "'Apple SD Gothic Neo'",
    "'Malgun Gothic'",
    "sans-serif",
  ]
    .filter(Boolean)
    .join(", ");
