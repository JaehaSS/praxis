import type * as Monaco from "monaco-editor";
// @ts-expect-error Monaco's bundled basic grammars do not publish declarations.
import { language as builtinPython } from "monaco-editor/esm/vs/basic-languages/python/python.js";

const root = builtinPython.tokenizer.root;
// Monaco 0.55.1 keeps the generic identifier fallback last; prepend symbol rules before it.
const defaultIdentifierRule = root[root.length - 1];

export const pythonHighlighting: Monaco.languages.IMonarchLanguage = {
  ...builtinPython,
  tokenizer: {
    ...builtinPython.tokenizer,
    root: [
      ...root.slice(0, -1),
      [/(class)(\s+)([a-zA-Z_]\w*)/, ["keyword", "white", "type.identifier"]],
      [/(async)(\s+)(def)(\s+)([a-zA-Z_]\w*)/, ["keyword", "white", "keyword", "white", "function"]],
      [/(def)(\s+)([a-zA-Z_]\w*)/, ["keyword", "white", "function"]],
      [
        /(False|None|True|and|as|assert|async|await|break|case|class|continue|def|del|elif|else|except|exec|finally|for|from|global|if|import|in|is|lambda|match|nonlocal|not|or|pass|print|raise|return|try|type|while|with|yield)(\s*)(?=\()/,
        ["keyword", "white"],
      ],
      [
        /([a-zA-Z_]\w*)(\s*)(?=\()/,
        ["function", "white"],
      ],
      defaultIdentifierRule,
    ],
  },
};

export function installPythonHighlighting(monaco: typeof Monaco) {
  monaco.languages.setMonarchTokensProvider("python", pythonHighlighting);
}
