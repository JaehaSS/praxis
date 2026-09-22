import { describe, expect, it } from "vitest";
import { capLines, MAX_LINES, parseAnsi } from "./ansi";

/** 세그먼트 텍스트만 이어붙여 비교하기 위한 헬퍼. */
const text = (lines: ReturnType<typeof parseAnsi>) =>
  lines.map((line) => line.map((segment) => segment.text).join(""));

describe("parseAnsi", () => {
  it("평범한 텍스트를 줄로 나눈다", () => {
    expect(text(parseAnsi("a\nb\nc"))).toEqual(["a", "b", "c"]);
    expect(text(parseAnsi("a\r\nb"))).toEqual(["a", "b"]);
  });

  it("빈 입력과 마지막 개행을 자연스럽게 다룬다", () => {
    expect(parseAnsi("")).toEqual([]);
    expect(text(parseAnsi("a\n"))).toEqual(["a"]);
  });

  it("색 코드를 세그먼트 스타일로 바꾼다", () => {
    const [line] = parseAnsi("\x1b[31mred\x1b[0m plain");
    expect(line[0]).toMatchObject({ text: "red", fg: "#e06c75" });
    expect(line[1]).toMatchObject({ text: " plain" });
    expect(line[1].fg).toBeUndefined();
  });

  it("굵기·흐림·밑줄과 해제를 처리한다", () => {
    const [line] = parseAnsi("\x1b[1mB\x1b[22mN\x1b[4mU\x1b[24mN2");
    expect(line[0]).toMatchObject({ text: "B", bold: true });
    expect(line[1].bold).toBeUndefined();
    expect(line[2]).toMatchObject({ text: "U", underline: true });
    expect(line[3].underline).toBeUndefined();
  });

  it("256색과 트루컬러를 해석한다", () => {
    const [c256] = parseAnsi("\x1b[38;5;196mX");
    expect(c256[0].fg).toBe("#ff0000");
    const [truecolor] = parseAnsi("\x1b[38;2;18;52;86mX");
    expect(truecolor[0].fg).toBe("#123456");
    const [gray] = parseAnsi("\x1b[38;5;232mX");
    expect(gray[0].fg).toBe("#080808");
  });

  it("배경색과 기본값 복귀를 처리한다", () => {
    const [line] = parseAnsi("\x1b[42mG\x1b[49mD");
    expect(line[0].bg).toBe("#98c379");
    expect(line[1].bg).toBeUndefined();
  });

  it("잘린 확장 색 시퀀스를 색으로 오해하지 않는다", () => {
    // 38 뒤에 모드가 없으면 남은 숫자를 파라미터로 소비해선 안 된다.
    const [line] = parseAnsi("\x1b[38mX");
    expect(line[0].fg).toBeUndefined();
  });

  it("캐리지 리턴은 그 줄을 처음부터 다시 쓴다", () => {
    // 진행률 표시가 남기는 잔상을 그대로 두면 로그가 읽히지 않는다.
    expect(text(parseAnsi("50%\r100%"))).toEqual(["100%"]);
    expect(text(parseAnsi("a\nlong\rshort"))).toEqual(["a", "short"]);
  });

  it("SGR이 아닌 CSI와 OSC를 버린다", () => {
    // 커서 이동·화면 지우기·타이틀 설정은 흐르는 로그에서 의미가 없다.
    expect(text(parseAnsi("a\x1b[2Kb"))).toEqual(["ab"]);
    expect(text(parseAnsi("a\x1b[10;20Hb"))).toEqual(["ab"]);
    expect(text(parseAnsi("a\x1b]0;title\x07b"))).toEqual(["ab"]);
  });

  it("제어문자는 표시하지 않되 탭은 남긴다", () => {
    expect(text(parseAnsi("a\x07b"))).toEqual(["ab"]);
    expect(text(parseAnsi("a\tb"))).toEqual(["a\tb"]);
  });

  it("스타일이 줄을 넘어 이어진다", () => {
    const lines = parseAnsi("\x1b[31mred\nstill red");
    expect(lines[0][0].fg).toBe("#e06c75");
    expect(lines[1][0].fg).toBe("#e06c75");
  });

  it("리셋 축약형(ESC[m)을 리셋으로 본다", () => {
    const [line] = parseAnsi("\x1b[31ma\x1b[mb");
    expect(line[1].fg).toBeUndefined();
  });
});

describe("capLines", () => {
  it("상한을 넘으면 앞에서 버린다", () => {
    const lines = parseAnsi(Array.from({ length: 10 }, (_, i) => `L${i}`).join("\n"));
    const capped = capLines(lines, 3);
    expect(text(capped)).toEqual(["L7", "L8", "L9"]);
  });

  it("상한 이하면 그대로 둔다", () => {
    const lines = parseAnsi("a\nb");
    expect(capLines(lines, 5)).toHaveLength(2);
    expect(MAX_LINES).toBeGreaterThan(0);
  });
});
