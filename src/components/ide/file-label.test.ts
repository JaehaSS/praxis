import { describe, expect, it } from "vitest";
import { splitLabel } from "./file-label";

describe("splitLabel", () => {
  it("확장자 사슬이 달라지면 꼬리도 달라진다 — 좁은 폭에서도 두 행이 구분된다", () => {
    expect(splitLabel("docker-compose.yml")).toEqual(["docker-compose", ".yml"]);
    expect(splitLabel("docker-compose.prod.yml")).toEqual(["docker-compose", ".prod.yml"]);
  });

  it("선두 점은 확장자 구분자가 아니다", () => {
    expect(splitLabel(".gitignore")).toEqual([".gitignore", ""]);
  });

  it("점이 없으면 꼬리도 없다", () => {
    expect(splitLabel("Makefile")).toEqual(["Makefile", ""]);
  });

  it("압축된 사슬 라벨은 마지막 폴더를 꼬리로 지킨다", () => {
    expect(splitLabel("src/main/java")).toEqual(["src/main", "/java"]);
  });

  it("사슬 라벨의 꼬리에는 길이 상한을 두지 않는다", () => {
    expect(splitLabel("src/generated_protobuf_bindings")).toEqual([
      "src",
      "/generated_protobuf_bindings",
    ]);
  });

  it("확장자 후보가 10자를 넘으면 버전 번호로 보고 마지막 점까지 물러선다", () => {
    expect(splitLabel("Praxis_0.1.0_aarch64.dmg")).toEqual(["Praxis_0.1.0_aarch64", ".dmg"]);
  });

  it("점이 하나면 그 점부터가 꼬리다", () => {
    expect(splitLabel("README.md")).toEqual(["README", ".md"]);
  });

  it("빈 문자열은 가르지 않는다", () => {
    expect(splitLabel("")).toEqual(["", ""]);
  });

  it("점으로 끝나도 머리가 남으면 가른다", () => {
    expect(splitLabel("archive.")).toEqual(["archive", "."]);
  });

  it("머리가 비게 되면 가르지 않는다", () => {
    expect(splitLabel("/java")).toEqual(["/java", ""]);
  });
});
