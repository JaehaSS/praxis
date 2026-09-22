import { describe, expect, it } from "vitest";
import { fileSize, fileTime, stamp } from "./fmt";

describe("fileSize", () => {
  it("1KB 미만은 바이트 그대로", () => {
    expect(fileSize(0)).toBe("0B");
    expect(fileSize(1023)).toBe("1023B");
  });

  it("단위가 올라가며 10 미만은 소수 한 자리", () => {
    expect(fileSize(1024)).toBe("1.0K");
    expect(fileSize(1536)).toBe("1.5K");
    expect(fileSize(1024 * 1024)).toBe("1.0M");
    expect(fileSize(3 * 1024 * 1024 * 1024)).toBe("3.0G");
  });

  it("10 이상은 반올림 정수", () => {
    expect(fileSize(12 * 1024)).toBe("12K");
    expect(fileSize(500 * 1024 * 1024)).toBe("500M");
  });

  it("최대 단위를 넘어도 T에서 멈춘다", () => {
    expect(fileSize(5 * 1024 ** 5)).toMatch(/T$/);
  });
});

describe("fileTime", () => {
  it("mtime이 없으면 빈 문자열", () => {
    expect(fileTime(0)).toBe("");
  });

  it("올해면 월-일 시:분", () => {
    const d = new Date();
    d.setMonth(0, 15);
    d.setHours(9, 5, 0, 0);
    expect(fileTime(d.getTime())).toBe("01-15 09:05");
  });

  it("해가 다르면 연-월-일", () => {
    const d = new Date();
    d.setFullYear(d.getFullYear() - 1, 10, 2);
    expect(fileTime(d.getTime())).toBe(`${d.getFullYear()}-11-02`);
  });
});

describe("stamp", () => {
  it("epoch초를 밀리초로 환산해 같은 표기를 낸다", () => {
    const d = new Date();
    d.setMonth(0, 15);
    d.setHours(9, 5, 0, 0);
    expect(stamp(Math.floor(d.getTime() / 1000))).toBe("01-15 09:05");
  });

  it("기록이 없는 0은 빈 문자열", () => {
    expect(stamp(0)).toBe("");
  });
});
