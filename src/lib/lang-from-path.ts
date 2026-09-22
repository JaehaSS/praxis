// 확장자 → 언어 id. monaco.ts에서 분리해 둔다 — 프롬프트 포맷터처럼 에디터를 띄우지 않는
// 모듈이 monaco-editor 번들(테마 정의·worker 등록 side effect 포함)을 끌어오지 않게 하기 위함.

/** 파일 확장자 → Monaco 언어 id. */
export function langFromPath(path: string): string {
  const ext = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  const map: Record<string, string> = {
    ts: "typescript",
    tsx: "typescript",
    mts: "typescript",
    cts: "typescript",
    js: "javascript",
    jsx: "javascript",
    mjs: "javascript",
    cjs: "javascript",
    rs: "rust",
    py: "python",
    pyi: "python",
    go: "go",
    java: "java",
    json: "json",
    css: "css",
    scss: "scss",
    html: "html",
    htm: "html",
    xhtml: "html",
    md: "markdown",
    markdown: "markdown",
    toml: "ini",
    yaml: "yaml",
    yml: "yaml",
    sh: "shell",
    sql: "sql",
    c: "c",
    h: "c",
    cpp: "cpp",
    hpp: "cpp",
  };
  return map[ext] ?? "plaintext";
}

/** 마크다운 코드펜스 info string. Monaco 전용 id인 `plaintext`만 `text`로 바꾼다. */
export function fenceLangFromPath(path: string): string {
  const lang = langFromPath(path);
  return lang === "plaintext" ? "text" : lang;
}
