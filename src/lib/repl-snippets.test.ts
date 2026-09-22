import { describe, expect, it } from "vitest";
import { absoluteWorktreePath, pandasOpenSnippet, pythonStringLiteral } from "./repl-snippets";

describe("Python 콘솔 코드 조각", () => {
  it("워크트리 상대 경로를 절대 경로로 잇는다", () => {
    expect(absoluteWorktreePath("/wt", "data/a.parquet")).toBe("/wt/data/a.parquet");
    expect(absoluteWorktreePath("/wt/", "data/a.parquet")).toBe("/wt/data/a.parquet");
  });

  it("이미 절대 경로면(작업 폴더 밖 탭) 그대로 둔다", () => {
    expect(absoluteWorktreePath("/wt", "/elsewhere/a.parquet")).toBe("/elsewhere/a.parquet");
  });

  it("따옴표·역슬래시·한글이 든 경로도 Python 문자열로 안전하다", () => {
    expect(pythonStringLiteral('a"b\\c 데이터')).toBe('"a\\"b\\\\c 데이터"');
  });

  it("pandas로 여는 셀은 df를 마지막 줄에 두어 Out[]로 표가 그려진다", () => {
    const code = pandasOpenSnippet("/wt", "data/판매.parquet");
    expect(code.split("\n")).toEqual([
      "import pandas as pd",
      'df = pd.read_parquet("/wt/data/판매.parquet")',
      "df",
    ]);
  });
});
