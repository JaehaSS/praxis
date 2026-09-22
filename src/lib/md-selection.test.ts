// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from "vitest";
import { lineOfNode, linesOfRange } from "./md-selection";

let host: HTMLElement;

beforeEach(() => {
  host = document.createElement("div");
  host.innerHTML = `
    <p data-md-line="3" data-md-line-end="4">첫 <em>문단</em></p>
    <ul data-md-line="6" data-md-line-end="8">
      <li data-md-line="6" data-md-line-end="6">하나</li>
      <li data-md-line="7" data-md-line-end="8">둘</li>
    </ul>
    <div id="맨몸">표식 없음</div>
  `;
  document.body.appendChild(host);
});

const textIn = (selector: string): Node => {
  const el = host.querySelector(selector);
  if (el == null) throw new Error(`${selector}를 찾지 못했다`);
  return el.firstChild ?? el;
};

describe("줄 되찾기", () => {
  it("텍스트 노드에서 위로 올라가 블록의 줄을 찾는다", () => {
    expect(lineOfNode(textIn("p"), "start")).toBe(3);
    expect(lineOfNode(textIn("p"), "end")).toBe(4);
  });

  it("가장 가까운 블록이 이긴다 — 목록 항목은 목록 전체가 아니다", () => {
    expect(lineOfNode(textIn("li:nth-child(2)"), "start")).toBe(7);
    expect(lineOfNode(textIn("li:nth-child(2)"), "end")).toBe(8);
  });

  it("강조 안쪽에서 골라도 문단까지 올라간다", () => {
    expect(lineOfNode(textIn("em"), "start")).toBe(3);
  });

  it("표식이 없으면 null", () => {
    expect(lineOfNode(textIn("#맨몸"), "start")).toBeNull();
  });
});

describe("선택 범위", () => {
  const rangeOf = (from: Node, to: Node): Range => {
    const range = document.createRange();
    range.setStart(from, 0);
    range.setEnd(to, 0);
    return range;
  };

  it("두 블록에 걸친 선택은 앞 블록의 시작부터 뒤 블록의 끝까지", () => {
    expect(linesOfRange(rangeOf(textIn("p"), textIn("li:nth-child(2)")))).toEqual({
      startLine: 3,
      endLine: 8,
    });
  });

  it("한쪽이 표식 없는 자리면 다른 쪽으로 메운다", () => {
    expect(linesOfRange(rangeOf(textIn("p"), textIn("#맨몸")))).toEqual({
      startLine: 3,
      endLine: 3,
    });
  });

  it("양쪽 다 표식이 없으면 null — 첨부할 좌표가 없다", () => {
    expect(linesOfRange(rangeOf(textIn("#맨몸"), textIn("#맨몸")))).toBeNull();
  });
});
