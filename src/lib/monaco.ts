// Monaco를 로컬 번들로 구성 (CDN 미사용 — local-first 원칙).
// Vite의 `?worker` import로 언어 워커를 별도 청크로 분리해 메인 스레드 차단 방지.
import * as monaco from "monaco-editor";
import { loader } from "@monaco-editor/react";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import cssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import htmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import tsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";
import { installPythonHighlighting } from "./python-highlighting";
import { getActiveTheme, subscribeTheme, THEMES, type Theme } from "./themes";

(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
  getWorker(_workerId, label) {
    if (label === "json") return new jsonWorker();
    if (label === "css" || label === "scss" || label === "less") return new cssWorker();
    if (label === "html" || label === "handlebars" || label === "razor") return new htmlWorker();
    if (label === "typescript" || label === "javascript") return new tsWorker();
    return new editorWorker();
  },
};

const hex = (c: string) => c.replace("#", "");

// 앱 색 토큰에 맞춘 표면색 + 테마가 선언한 구문색.
// Monaco 테마 이름 = themes.ts의 테마 id — EditorPane이 활성 테마 id를 그대로 넘긴다.
/** 같은 이름 defineTheme은 덮어쓰기이므로 커스텀/드래프트 재정의에도 그대로 쓴다. */
export function defineMonacoTheme(theme: Theme) {
  const t = theme.tokens;
  const s = theme.syntax;
  const pythonFunction = theme.kind === "dark" ? "DCDCAA" : "795E26";
  monaco.editor.defineTheme(theme.id, {
    base: theme.kind === "dark" ? "vs-dark" : "vs",
    inherit: true,
    // syntax 없는 테마는 Monaco 기본색을 상속하되 Python 함수만 구분한다.
    // 기본 Praxis 2종은 syntax 팔레트가 없으므로 이 규칙이 def·호출 이름을 분리한다.
    //
    // syntax 팔레트에서는 `identifier`도 명시해 색의 출처를 테마로 고정한다.
    // 팔레트 없는 테마의 identifier는 Monaco 본문색을 그대로 상속한다.
    rules: s
      ? [
          { token: "keyword", foreground: hex(s.keyword) },
          { token: "string", foreground: hex(s.string) },
          { token: "string.escape", foreground: hex(s.string) },
          { token: "number", foreground: hex(s.number) },
          { token: "comment", foreground: hex(s.comment) },
          { token: "comment.doc", foreground: hex(s.comment) },
          { token: "type", foreground: hex(s.type) },
          { token: "type.identifier", foreground: hex(s.type) },
          { token: "namespace", foreground: hex(s.type) },
          { token: "function", foreground: hex(s.func) },
          { token: "annotation", foreground: hex(s.func) },
          { token: "variable", foreground: hex(s.variable) },
          { token: "variable.predefined", foreground: hex(s.variable) },
          { token: "constant", foreground: hex(s.constant) },
          { token: "regexp", foreground: hex(s.constant) },
          { token: "operator", foreground: hex(s.operator) },
          { token: "delimiter", foreground: hex(s.operator) },
          { token: "tag", foreground: hex(s.tag) },
          { token: "attribute.name", foreground: hex(s.tag) },
          { token: "metatag", foreground: hex(s.tag) },
          // Monarch가 언어마다 다른 이름으로 내보내는 것들. 여기 없으면 그 언어에서만
          // 색이 빠져 "TS는 되는데 Rust는 안 된다" 같은 증상이 된다.
          { token: "identifier", foreground: hex(s.variable) },
          { token: "entity.name.function", foreground: hex(s.func) },
          { token: "support.function", foreground: hex(s.func) },
          { token: "entity.name.type", foreground: hex(s.type) },
          { token: "entity.name.type.class", foreground: hex(s.type) },
          { token: "entity.name.type.interface", foreground: hex(s.type), fontStyle: "italic" },
          { token: "support.type", foreground: hex(s.type) },
          { token: "storage.type", foreground: hex(s.keyword) },
          { token: "keyword.control", foreground: hex(s.keyword) },
          { token: "constant.language", foreground: hex(s.constant) },
          { token: "constant.numeric", foreground: hex(s.number) },
          { token: "variable.parameter", foreground: hex(s.variable), fontStyle: "italic" },
          { token: "variable.other.constant", foreground: hex(s.constant) },
        ]
      : [{ token: "function.python", foreground: pythonFunction }],
    colors: {
      "editor.background": t.bg,
      "editor.foreground": t.text,
      "editorLineNumber.foreground": t.textMuted,
      "editorLineNumber.activeForeground": t.text2,
      "editorGutter.background": t.bg,
      "editorIndentGuide.background1": t.border,
      // 선택 배경은 내장 테마 값을 그대로 둔다 — 표면색보다 대비 요구가 까다롭고,
      // 팔레트에서 파생한 회색은 선택 영역을 오히려 흐리게 만든다.
      "editorWidget.background": t.raised,
      "editorWidget.border": t.border,
    },
  });
}

for (const theme of THEMES) defineMonacoTheme(theme);
// 커스텀·드래프트 테마는 로드 시점에 없다 — 활성 전환 때마다 (재)정의한다.
// EditorPane 렌더는 subscribe 콜백 뒤에 오므로 정의가 항상 선행된다.
subscribeTheme(() => defineMonacoTheme(getActiveTheme()));
defineMonacoTheme(getActiveTheme());

// @monaco-editor/react가 번들된 monaco를 쓰도록 (기본 CDN 로더 대체).
loader.config({ monaco });
installPythonHighlighting(monaco);

// 언어 판별은 lang-from-path.ts가 소유한다(monaco 의존 없는 순수 모듈) — 기존 import 경로 보존을 위해 재수출.
export { langFromPath } from "./lang-from-path";

export { monaco };
