// @vitest-environment jsdom

import type * as Monaco from "monaco-editor";
import { beforeAll, describe, expect, it } from "vitest";
import { langFromPath } from "./lang-from-path";
import { editor, languages } from "monaco-editor/esm/vs/editor/editor.api.js";

type Token = { offset: number; type: string };

function tokenTypeAt(line: string, word: string, tokens: Token[]): string | undefined {
  const offset = line.indexOf(word);
  const token = tokens.find((candidate, index) => {
    const end = tokens[index + 1]?.offset ?? line.length;
    return candidate.offset <= offset && offset < end;
  });

  return token?.type;
}

describe("Python symbol highlighting", () => {
  let pythonHighlighting: Monaco.languages.IMonarchLanguage;

  beforeAll(async () => {
    Object.defineProperty(document, "queryCommandSupported", { value: () => false });
    Object.defineProperty(globalThis, "CSS", { value: { escape: (value: string) => value } });
    Object.defineProperty(window, "matchMedia", {
      value: () => ({ addEventListener() {}, matches: false, removeEventListener() {} }),
    });
    const highlighting = await import("./python-highlighting");

    languages.register({ id: "python" });
    highlighting.installPythonHighlighting({ languages } as typeof Monaco);
    pythonHighlighting = highlighting.pythonHighlighting;
  });

  it("keeps the Monaco 0.55.1 identifier fallback as the augmentation anchor", () => {
    const root = pythonHighlighting.tokenizer.root as [RegExp, { cases: Record<string, string> }][];
    const fallback = root[root.length - 1];

    expect(fallback[0].source).toBe("[a-zA-Z_]\\w*");
    expect(fallback[1]).toMatchObject({
      cases: { "@default": "identifier", "@keywords": "keyword" },
    });
  });

  it("distinguishes class, declaration, and call names from ordinary identifiers", () => {
    const source = [
      "class Greeter:",
      "    async def greet(name):",
      "        result = self.format (name)",
      "        return result",
    ];
    const tokens = editor.tokenize(source.join("\n"), "python") as Token[][];

    expect(tokenTypeAt(source[0], "Greeter", tokens[0])).toContain("type.identifier");
    expect(tokenTypeAt(source[1], "greet", tokens[1])).toContain("function");
    expect(tokenTypeAt(source[2], "format", tokens[2])).toContain("function");
    expect(tokenTypeAt(source[2], "result", tokens[2])).toContain("identifier");
    expect(tokenTypeAt(source[3], "return", tokens[3])).toContain("keyword");
  });

  it("preserves Python states while handling incomplete declarations and calls", () => {
    const source = [
      "@decorate",
      "class Greeter:",
      "    async def greet(name):",
      "        \"\"\"format()",
      "        class Hidden:",
      "        \"\"\"",
      "        text = r\"format()\" # render()",
      "        value = self.format (name)",
      "        if (",
      "        def incomplete",
      "        return value",
    ];
    const tokens = editor.tokenize(source.join("\n"), "python") as Token[][];

    expect(tokenTypeAt(source[0], "decorate", tokens[0])).toContain("tag");
    expect(tokenTypeAt(source[4], "Hidden", tokens[4])).toContain("string");
    expect(tokenTypeAt(source[6], "format", tokens[6])).toContain("string");
    expect(tokenTypeAt(source[6], "render", tokens[6])).toContain("comment");
    expect(tokenTypeAt(source[7], "format", tokens[7])).toContain("function");
    expect(tokenTypeAt(source[8], "if", tokens[8])).toContain("keyword");
    expect(tokenTypeAt(source[9], "incomplete", tokens[9])).toContain("function");
    expect(tokenTypeAt(source[10], "value", tokens[10])).toContain("identifier");
    expect(langFromPath("src/main.py")).toBe("python");
    expect(langFromPath("types/api.pyi")).toBe("python");
  });
});
