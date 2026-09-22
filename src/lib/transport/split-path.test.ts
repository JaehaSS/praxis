import { describe, expect, it } from "vitest";
import { splitPath } from "./runner";

describe("splitPath", () => {
  it("절대 경로를 부모 디렉터리와 파일명으로 나눈다", () => {
    expect(splitPath("/home/u/work/a.txt")).toEqual({ dir: "/home/u/work", name: "a.txt" });
  });

  it("루트 직하 파일의 부모는 /", () => {
    expect(splitPath("/etc")).toEqual({ dir: "/", name: "etc" });
  });

  it("윈도우 구분자를 슬래시로 정규화한다", () => {
    expect(splitPath("C:\\Users\\me\\a.txt")).toEqual({ dir: "C:/Users/me", name: "a.txt" });
  });

  it("끝의 슬래시는 무시한다", () => {
    expect(splitPath("/home/u/work/")).toEqual({ dir: "/home/u", name: "work" });
  });

  it("구분자가 없으면 현재 디렉터리 기준", () => {
    expect(splitPath("a.txt")).toEqual({ dir: ".", name: "a.txt" });
  });
});
