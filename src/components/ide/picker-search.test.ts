import { describe, expect, it } from "vitest";
import { filterByQuery, matchRanges, queryTokens } from "./picker-search";

const REPOS = ["/Users/me/work/praxis", "/Users/me/archive/praxis", "/Users/me/work/praxis-mobile"];
const MODELS = [
  { id: "claude-opus-5", label: "Claude Opus 5" },
  { id: "claude-opus-5[1m]", label: "Claude Opus 5 · 1M 컨텍스트" },
  { id: "claude-sonnet-5", label: "Claude Sonnet 5" },
];

describe("queryTokens", () => {
  it("공백으로 쪼개고 소문자로 내린다", () => {
    expect(queryTokens("  JH2   Tree ")).toEqual(["jh2", "tree"]);
  });

  it("공백뿐이면 토큰이 없다", () => {
    expect(queryTokens("   ")).toEqual([]);
  });
});

describe("filterByQuery", () => {
  it("토큰을 모두 포함하는 후보만 남긴다", () => {
    expect(filterByQuery(REPOS, "work praxis", (p) => p)).toEqual([
      "/Users/me/work/praxis",
      "/Users/me/work/praxis-mobile",
    ]);
  });

  it("레포는 경로 조각으로 찾힌다 — basename이 같아도 갈린다", () => {
    expect(filterByQuery(REPOS, "archive", (p) => p)).toEqual(["/Users/me/archive/praxis"]);
  });

  it("모델은 label과 id 둘 다 대상이다", () => {
    expect(filterByQuery(MODELS, "sonnet", (m) => `${m.label} ${m.id}`).map((m) => m.id)).toEqual([
      "claude-sonnet-5",
    ]);
    expect(filterByQuery(MODELS, "1m", (m) => `${m.label} ${m.id}`).map((m) => m.id)).toEqual([
      "claude-opus-5[1m]",
    ]);
  });

  it("대소문자를 가리지 않는다", () => {
    expect(filterByQuery(MODELS, "OPUS", (m) => m.id)).toHaveLength(2);
  });

  it("원래 순서를 재정렬하지 않는다", () => {
    expect(filterByQuery(MODELS, "claude", (m) => m.id).map((m) => m.id)).toEqual([
      "claude-opus-5",
      "claude-opus-5[1m]",
      "claude-sonnet-5",
    ]);
  });

  it("질의가 비면 전부 그대로 돌려준다 — 조용히 자르지 않는다", () => {
    expect(filterByQuery(REPOS, "   ", (p) => p)).toHaveLength(3);
  });
});

describe("matchRanges", () => {
  it("토큰마다 첫 매치만 잡는다", () => {
    expect(matchRanges("a-popout-popout", ["popout"])).toEqual([[2, 8]]);
  });

  it("겹치는 구간을 하나로 합친다", () => {
    expect(matchRanges("popout", ["pop", "opo"])).toEqual([[0, 4]]);
  });

  it("없는 토큰은 구간을 만들지 않는다", () => {
    expect(matchRanges("praxis", ["zzz"])).toEqual([]);
  });
});
