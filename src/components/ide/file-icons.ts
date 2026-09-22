import type { IconName } from "./icons";
import type { FileGlyphName } from "./file-glyphs";

/** 파일 이름 → 트리에 그릴 아이콘.
 *
 * 종류를 가르는 주 채널은 **형태**다. 실루엣만으로 코드/설정/문서/자산이 갈리고, 그 위에
 * 저채도 틴트를 보조로만 얹는다(2026-08 #97) — 전부 muted 회색이면 디렉터리와 파일조차
 * 스캔이 안 된다. 틴트는 상태색·액센트와 hue가 겹치지 않는 고정 팔레트라 상태 신호를
 * 묻지 않는다. 값의 정본은 index.css(`--ft-*`)와 DESIGN.md.
 *
 * 확장자보다 **파일명 전체 일치가 우선**이다. `Cargo.lock`은 확장자 `lock`이지만
 * `.gitignore`는 확장자가 없고 `Dockerfile`은 점 자체가 없다 — 이름 규칙이 먼저다.
 */

/** 이름 그대로 알아보는 파일들 — 확장자 규칙보다 먼저 본다. */
const BY_NAME: Record<string, IconName> = {
  dockerfile: "database",
  makefile: "terminal",
  "cargo.lock": "lock",
  "package-lock.json": "lock",
  "yarn.lock": "lock",
  "pnpm-lock.yaml": "lock",
  "bun.lockb": "lock",
  ".gitignore": "branch",
  ".gitattributes": "branch",
  ".gitmodules": "branch",
  ".env": "lock",
  license: "fileText",
};

const BY_EXT: Record<string, IconName> = {
  // 코드
  ts: "fileCode",
  tsx: "fileCode",
  mts: "fileCode",
  cts: "fileCode",
  js: "fileCode",
  jsx: "fileCode",
  mjs: "fileCode",
  cjs: "fileCode",
  rs: "fileCode",
  py: "fileCode",
  go: "fileCode",
  java: "fileCode",
  kt: "fileCode",
  swift: "fileCode",
  rb: "fileCode",
  php: "fileCode",
  c: "fileCode",
  h: "fileCode",
  cc: "fileCode",
  cpp: "fileCode",
  hpp: "fileCode",
  cs: "fileCode",
  sql: "database",
  // 셸
  sh: "terminal",
  bash: "terminal",
  zsh: "terminal",
  fish: "terminal",
  ps1: "terminal",
  // 설정·데이터
  json: "braces",
  jsonc: "braces",
  yaml: "braces",
  yml: "braces",
  toml: "braces",
  ini: "braces",
  conf: "braces",
  cfg: "braces",
  lock: "lock",
  // 마크업·스타일
  html: "code",
  htm: "code",
  xml: "code",
  svg: "image",
  css: "palette",
  scss: "palette",
  sass: "palette",
  less: "palette",
  // 문서
  md: "markdown",
  markdown: "markdown",
  mdx: "markdown",
  txt: "fileText",
  rst: "fileText",
  pdf: "fileText",
  csv: "chart",
  parquet: "chart",
  // 자산
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  avif: "image",
  ico: "image",
  bmp: "image",
  // 폰트·바이너리
  woff: "database",
  woff2: "database",
  ttf: "database",
  otf: "database",
  wasm: "database",
  db: "database",
  sqlite: "database",
};

/** 파일 하나에 붙일 아이콘. 아는 규칙이 없으면 무지(無地) 파일로 떨어진다. */
export function fileIcon(name: string): IconName {
  const lower = name.toLowerCase();
  const byName = BY_NAME[lower];
  if (byName) return byName;

  // 이름에 점이 없거나 점으로만 시작하면(.gitignore 등) 확장자가 없는 것으로 본다.
  const dot = lower.lastIndexOf(".");
  if (dot <= 0) return "file";
  return BY_EXT[lower.slice(dot + 1)] ?? "file";
}

/** 아이콘 → 틴트 클래스. 이름이 아니라 **아이콘**에서 파생한다 — 실루엣이 이미 카테고리를
 *  인코딩하므로, 여기서 다시 확장자를 분류하면 두 규칙이 어긋날 자리가 생긴다.
 *  규칙이 없는 아이콘은 무색(undefined) — 호출부가 muted로 떨어뜨린다. */
const TINT: Partial<Record<IconName, string>> = {
  folder: "text-ft-folder",
  folderOpen: "text-ft-folder",
  fileCode: "text-ft-code",
  terminal: "text-ft-code",
  markdown: "text-ft-doc",
  fileText: "text-ft-doc",
  chart: "text-ft-doc",
  palette: "text-ft-style",
  image: "text-ft-style",
  code: "text-ft-style", // html/xml 마크업
  braces: "text-ft-data",
  database: "text-ft-data",
};

export const iconTint = (icon: IconName): string | undefined => TINT[icon];

/** 디렉터리 아이콘 — 펼침 여부로 실루엣을 바꾼다. */
export const folderIcon = (open: boolean): IconName => (open ? "folderOpen" : "folder");

/** 도트 파일/디렉터리 — 숨김 토글의 판정 기준. */
export const isHiddenName = (name: string) => name.startsWith(".");

// ── 언어별 컬러 글리프 (material-icon-theme, MIT) ───────────────────────────
//
// 위의 단색 아이콘 체계는 **그대로 남는다.** 여기 규칙이 없는 파일은 그쪽으로 후퇴하고,
// 그때 `iconTint`가 저채도 틴트를 얹는다 — "형태가 주 채널"이라는 계약이 유지된다.

/** 이름 그대로 알아보는 파일 — 확장자 규칙보다 먼저 본다(`BY_NAME`과 같은 우선순위). */
const GLYPH_BY_NAME: Record<string, FileGlyphName> = {
  dockerfile: "docker",
  "docker-compose.yml": "docker",
  "docker-compose.yaml": "docker",
  ".gitignore": "git",
  ".gitattributes": "git",
  ".gitmodules": "git",
  "package.json": "npm",
  "package-lock.json": "npm",
  "cargo.lock": "lock",
  "yarn.lock": "lock",
  "pnpm-lock.yaml": "lock",
  "bun.lockb": "lock",
  "tailwind.config.js": "tailwindcss",
  "tailwind.config.ts": "tailwindcss",
  "vite.config.ts": "vite",
  "vite.config.js": "vite",
  "go.mod": "go",
  "go.sum": "go",
  "tsconfig.json": "settings",
  "tsconfig.node.json": "settings",
  ".editorconfig": "settings",
  "postcss.config.js": "settings",
};

/**
 * 파일명 **접미**로 알아보는 것들 — 테스트 파일은 VS Code도 따로 표시한다.
 * 전체 일치보다는 뒤, 확장자보다는 앞이다.
 */
const GLYPH_BY_SUFFIX: [string, FileGlyphName][] = [
  [".test.ts", "test-ts"],
  [".test.tsx", "test-ts"],
  [".spec.ts", "test-ts"],
  [".behavior.test.tsx", "test-ts"],
  [".test.js", "test-js"],
  [".test.jsx", "test-js"],
  [".spec.js", "test-js"],
];

const GLYPH_BY_EXT: Record<string, FileGlyphName> = {
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "react",
  jsx: "react",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  rs: "rust",
  py: "python",
  go: "go",
  java: "java",
  kt: "kotlin",
  swift: "swift",
  rb: "ruby",
  php: "php",
  c: "c",
  h: "c",
  cc: "cpp",
  cpp: "cpp",
  hpp: "cpp",
  cs: "csharp",
  html: "html",
  htm: "html",
  xml: "xml",
  css: "css",
  scss: "sass",
  sass: "sass",
  less: "sass",
  json: "json",
  jsonc: "json",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  md: "markdown",
  markdown: "markdown",
  mdx: "markdown",
  sql: "database",
  db: "database",
  sqlite: "database",
  sh: "console",
  bash: "console",
  zsh: "console",
  fish: "console",
  ps1: "console",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  avif: "image",
  ico: "image",
  bmp: "image",
  svg: "image",
  woff: "font",
  woff2: "font",
  ttf: "font",
  otf: "font",
  txt: "document",
  rst: "document",
  pdf: "document",
};

/**
 * 파일 하나의 컬러 글리프. 아는 규칙이 없으면 `null` — 호출부가 단색 아이콘으로 후퇴한다.
 *
 * `fileIcon`과 **같은 우선순위**를 쓴다(이름 전체 일치 → 확장자). 두 함수가 다른 순서를
 * 쓰면 같은 파일이 아이콘과 틴트에서 다른 종류로 분류되는 자리가 생긴다.
 */
export function fileGlyph(name: string): FileGlyphName | null {
  const lower = name.toLowerCase();
  const byName = GLYPH_BY_NAME[lower];
  if (byName) return byName;

  // 가장 긴 접미가 이긴다 — `.behavior.test.tsx`가 `.test.tsx`보다 먼저 잡혀야 한다.
  const suffix = GLYPH_BY_SUFFIX.filter(([end]) => lower.endsWith(end)).sort(
    (a, b) => b[0].length - a[0].length,
  )[0];
  if (suffix) return suffix[1];

  const dot = lower.lastIndexOf(".");
  if (dot <= 0) return null;
  return GLYPH_BY_EXT[lower.slice(dot + 1)] ?? null;
}
