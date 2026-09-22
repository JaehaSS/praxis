import { describe, expect, it } from "vitest";
import { codeQuickOpenItems, parseCodeItemId } from "./quickopen";

const hits = [
  { path: "src/a.ts", line: 12, column: 5, text: "  const needle = 1;" },
  { path: "src/b.rs", line: 3, column: 1, text: "// needle" },
];

describe("code 스코프", () => {
  it("경로:줄을 제목으로, 매치 줄을 부제로 준다", () => {
    const items = codeQuickOpenItems(hits);
    expect(items[0].title).toBe("src/a.ts:12");
    expect(items[0].subtitle).toBe("const needle = 1;");
  });

  it("백엔드 순서를 그대로 보존한다 (재정렬하지 않는다)", () => {
    const items = codeQuickOpenItems(hits);
    expect(items.map((i) => i.title)).toEqual(["src/a.ts:12", "src/b.rs:3"]);
  });

  it("id에서 좌표를 되찾는다", () => {
    const id = codeQuickOpenItems(hits)[0].id;
    expect(parseCodeItemId(id)).toEqual({ path: "src/a.ts", line: 12, column: 5 });
  });

  it("경로에 콜론이 있어도 뒤에서 잘라 좌표를 찾는다", () => {
    const id = codeQuickOpenItems([{ path: "a:b/c.ts", line: 2, column: 3, text: "x" }])[0].id;
    expect(parseCodeItemId(id)).toEqual({ path: "a:b/c.ts", line: 2, column: 3 });
  });

  it("형식이 아니면 null", () => {
    expect(parseCodeItemId("src/a.ts")).toBeNull();
  });
});
