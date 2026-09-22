import { describe, expect, it } from "vitest";
import {
  SETTINGS_CATALOG,
  SETTINGS_TABS,
  searchSettings,
  tabLabel,
} from "./settings-catalog";

describe("설정 카탈로그", () => {
  it("id가 유일하다 — 검색 결과가 두 자리를 가리키면 스크롤이 어디로 갈지 정해지지 않는다", () => {
    const ids = SETTINGS_CATALOG.map((entry) => entry.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("모든 항목이 실재하는 탭에 속한다", () => {
    const tabs = new Set(SETTINGS_TABS.map((spec) => spec.key));
    for (const entry of SETTINGS_CATALOG) {
      expect(tabs.has(entry.tab), `${entry.id} → ${entry.tab}`).toBe(true);
    }
  });

  it("탭 키가 유일하고 라벨이 비어 있지 않다", () => {
    const keys = SETTINGS_TABS.map((spec) => spec.key);
    expect(new Set(keys).size).toBe(keys.length);
    for (const spec of SETTINGS_TABS) expect(spec.label.trim()).not.toBe("");
  });
});

describe("설정 검색", () => {
  it("빈 질의는 아무것도 내지 않는다 — 전체 목록을 쏟으면 검색이 아니다", () => {
    expect(searchSettings("")).toEqual([]);
    expect(searchSettings("   ")).toEqual([]);
  });

  it("라벨 선두 일치가 포함 일치보다 위에 온다", () => {
    const hits = searchSettings("테마");
    expect(hits[0]?.id).toBe("theme");
  });

  it("라벨에 없는 말도 키워드로 찾는다", () => {
    expect(searchSettings("worktree").map((entry) => entry.id)).toContain("use-worktree");
    expect(searchSettings("사용량").map((entry) => entry.id)).toContain("agent-cli");
  });

  it("설명만 걸리는 질의도 찾되 라벨·키워드 뒤로 밀린다", () => {
    const hits = searchSettings("폰트");
    expect(hits[0]?.id).toBe("font");
  });

  it("없는 말은 빈 결과다", () => {
    expect(searchSettings("존재하지않는설정이름")).toEqual([]);
  });

  it("결과 수가 상한을 넘지 않는다", () => {
    expect(searchSettings("ㅇ", 3).length).toBeLessThanOrEqual(3);
  });

  it("탭 라벨은 카탈로그에서 나온다", () => {
    expect(tabLabel("run")).toBe("작업 실행");
  });
});
