import { describe, it, expect } from "vitest";
import { matchMentionToken, filterMentionFiles, applyMention, filterSkills } from "./mention";

describe("matchMentionToken", () => {
  it("줄머리 @는 빈 토큰을 매치한다", () => expect(matchMentionToken("@")).toBe(""));
  it("공백 뒤 @토큰을 매치한다", () => expect(matchMentionToken("hi @src")).toBe("src"));
  it("이메일처럼 문자 뒤 @는 매치하지 않는다", () => expect(matchMentionToken("a@b")).toBeNull());
  it("@ 뒤 공백이 오면 종료된 것으로 본다", () => expect(matchMentionToken("@src ")).toBeNull());
});

describe("filterMentionFiles", () => {
  const files = ["src/App.tsx", "src/lib/ipc.ts", "README.md"];
  it("대소문자 무시 substring으로 필터", () =>
    expect(filterMentionFiles(files, "IPC")).toEqual(["src/lib/ipc.ts"]));
  it("빈 토큰은 전체(최대 limit) 반환", () =>
    expect(filterMentionFiles(files, "", 2)).toEqual(["src/App.tsx", "src/lib/ipc.ts"]));
});

describe("applyMention", () => {
  it("캐럿 앞 @토큰을 `@path `로 치환하고 새 캐럿을 반환", () => {
    const r = applyMention("see @Ap rest", 7, "src/App.tsx");
    expect(r.value).toBe("see @src/App.tsx  rest");
    expect(r.caret).toBe(17);
  });

  it("path에 $-시퀀스가 있어도 치환 패턴이 아닌 리터럴로 삽입한다", () => {
    // 정규식 캡처그룹($1)·$& 등이 replace 특수문자로 해석되면 경로가 손상됨.
    const r = applyMention("@ty", 3, "src/$types.ts");
    expect(r.value).toBe("@src/$types.ts ");
  });
});

describe("filterSkills", () => {
  const skills = [
    { name: "feature-development" },
    { name: "fable-advisor-worker" },
    { name: "codex:rescue" },
    { name: "verification-loop" },
  ];

  it("이름 앞부분으로 좁힌다 — /fe는 feature-development로 간다", () =>
    expect(filterSkills(skills, "fe").map((s) => s.name)).toEqual(["feature-development"]));

  it("빈 프리픽스는 전부(최대 limit) 보여 준다", () =>
    expect(filterSkills(skills, "", 2)).toHaveLength(2));

  it("대소문자를 가리지 않는다", () =>
    expect(filterSkills(skills, "FE").map((s) => s.name)).toEqual(["feature-development"]));

  it("플러그인은 뒷마디로도 닿되 앞부분 일치 뒤에 온다", () => {
    const list = [{ name: "codex:rescue" }, { name: "resume-task" }];
    expect(filterSkills(list, "res").map((s) => s.name)).toEqual(["resume-task", "codex:rescue"]);
  });

  it("어디에도 걸리지 않으면 빈 목록", () =>
    expect(filterSkills(skills, "zzz")).toEqual([]));
});
