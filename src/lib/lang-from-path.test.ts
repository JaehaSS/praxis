import { describe, expect, it } from "vitest";

import { langFromPath } from "./lang-from-path";

describe("langFromPath", () => {
  it.each([
    ["src/module.mts", "typescript"],
    ["src/module.cts", "typescript"],
    ["src/module.mjs", "javascript"],
    ["src/module.cjs", "javascript"],
    ["types/model.pyi", "python"],
    ["src/Target.java", "java"],
  ])("maps %s to %s", (path, language) => {
    expect(langFromPath(path)).toBe(language);
  });

  it("keeps unsupported extensions as plaintext", () => {
    expect(langFromPath("src/notes.unknown")).toBe("plaintext");
  });
});
