import { describe, expect, it } from "vitest";
import { findTerminalFileLinks, logicalLineAt, type BufferLike } from "./terminal-file-links";

/** 문자열 배열을 xterm 버퍼처럼 보이게 감싼다. `~`로 시작하는 줄은 앞 줄에서 wrap된 것. */
function fakeBuffer(lines: string[], cols = 40): BufferLike {
  const rows = lines.map((raw) => ({
    wrapped: raw.startsWith("~"),
    chars: Array.from(raw.startsWith("~") ? raw.slice(1) : raw),
  }));
  return {
    length: rows.length,
    getLine(y) {
      const row = rows[y];
      if (!row) return undefined;
      // 한 문자가 몇 셀을 먹는지 계산해 실제 버퍼처럼 셀 배열을 만든다(한글 = 2셀).
      const cells: { chars: string; width: number }[] = [];
      for (const ch of row.chars) {
        const width = /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿︰-﹯]/.test(ch)
          ? 2
          : 1;
        cells.push({ chars: ch, width });
        if (width === 2) cells.push({ chars: "", width: 0 });
      }
      while (cells.length < cols) cells.push({ chars: " ", width: 1 });
      return {
        length: cells.length,
        isWrapped: row.wrapped,
        getCell(x) {
          const cell = cells[x];
          return cell ? { getChars: () => cell.chars, getWidth: () => cell.width } : undefined;
        },
      };
    },
  };
}

describe("logicalLineAt", () => {
  it("maps each character to the cell it sits in, counting wide glyphs as two", () => {
    const { text, coords } = logicalLineAt(fakeBuffer(["한글 a/b.ts"], 12), 0);

    expect(text.trimEnd()).toBe("한글 a/b.ts");
    expect(coords[0]).toEqual({ x: 0, y: 0, width: 2 });
    expect(coords[1]).toEqual({ x: 2, y: 0, width: 2 });
    // 'a'는 문자 인덱스 3이지만 셀은 x=5 — 한글 두 자가 4셀을 먹었다.
    expect(coords[3]).toEqual({ x: 5, y: 0, width: 1 });
  });

  it("rejoins a wrapped line from any of its rows", () => {
    const buffer = fakeBuffer(["저장: docs/plans/0014", "~-partition.md 끝"], 21);

    for (const row of [0, 1]) {
      expect(logicalLineAt(buffer, row).text).toContain("docs/plans/0014-partition.md");
    }
  });
});

describe("findTerminalFileLinks", () => {
  it("puts the link on the cells the path occupies, not the character indexes", () => {
    const [link] = findTerminalFileLinks(fakeBuffer(["저장 완료: docs/a.md"], 24), 1);

    expect(link.text).toBe("docs/a.md");
    // "저장 완료: " = 2+2+1+2+2+1+1 = 11셀 → 경로는 12번째 셀에서 시작(1-based).
    expect(link.range.start).toEqual({ x: 12, y: 1 });
    expect(link.range.end).toEqual({ x: 20, y: 1 });
  });

  it("spans both rows of a wrapped path", () => {
    const links = findTerminalFileLinks(fakeBuffer(["보라 docs/plans/aa", "~bb/cc.md 를"], 18), 1);

    expect(links).toHaveLength(1);
    expect(links[0].text).toBe("docs/plans/aabb/cc.md");
    expect(links[0].range.start.y).toBe(1);
    expect(links[0].range.end.y).toBe(2);
  });

  it("keeps the line suffix and finds every path on the row", () => {
    const links = findTerminalFileLinks(fakeBuffer(["src/App.tsx:1187 와 src/lib.rs"], 40), 1);

    expect(links.map((link) => link.text)).toEqual(["src/App.tsx:1187", "src/lib.rs"]);
  });

  it("returns nothing for a row without a path", () => {
    expect(findTerminalFileLinks(fakeBuffer(["빌드 완료 and/or 종료"], 30), 1)).toEqual([]);
  });
});
