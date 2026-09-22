import { describe, it, expect } from "vitest";
import {
  mergeQuickOpenResults,
  scoreQuickOpenItem,
  fileQuickOpenItems,
  QUICK_OPEN_COMMANDS,
  type QuickOpenItem,
} from "./quickopen";

const NOW = 1_700_000_000_000;
const DAY = 24 * 60 * 60 * 1000;

function item(partial: Partial<QuickOpenItem> & Pick<QuickOpenItem, "scope" | "id" | "title">): QuickOpenItem {
  return partial;
}

describe("scoreQuickOpenItem", () => {
  it("빈 쿼리는 매치 실패가 아니라 최근성 점수만 반환한다", () => {
    const recent = item({ scope: "task", id: "1", title: "fix login", updatedAt: NOW });
    expect(scoreQuickOpenItem(recent, "", NOW)).toBeGreaterThan(0);
  });

  it("제목 완전 일치가 접두/부분 일치보다 높은 점수를 받는다", () => {
    const exact = item({ scope: "task", id: "1", title: "deploy" });
    const prefix = item({ scope: "task", id: "2", title: "deploy staging" });
    const substring = item({ scope: "task", id: "3", title: "auto deploy job" });
    const exactScore = scoreQuickOpenItem(exact, "deploy", NOW)!;
    const prefixScore = scoreQuickOpenItem(prefix, "deploy", NOW)!;
    const substringScore = scoreQuickOpenItem(substring, "deploy", NOW)!;
    expect(exactScore).toBeGreaterThan(prefixScore);
    expect(prefixScore).toBeGreaterThan(substringScore);
  });

  it("제목·부제 어디에도 매치하지 않으면 null을 반환한다", () => {
    const it1 = item({ scope: "file", id: "a", title: "src/App.tsx", subtitle: "workspace" });
    expect(scoreQuickOpenItem(it1, "zzz-no-match", NOW)).toBeNull();
  });

  it("부제(subtitle) 일치는 제목 매치보다 낮은 가중을 받는다", () => {
    const titleMatch = item({ scope: "task", id: "1", title: "login bug" });
    const subtitleMatch = item({ scope: "task", id: "2", title: "dashboard", subtitle: "login flow repo" });
    const titleScore = scoreQuickOpenItem(titleMatch, "login", NOW)!;
    const subtitleScore = scoreQuickOpenItem(subtitleMatch, "login", NOW)!;
    expect(titleScore).toBeGreaterThan(subtitleScore);
  });
});

describe("mergeQuickOpenResults — 소스 병합·최근성·상한·스코프", () => {
  it("동일 매치 등급(둘 다 부분 일치)이면 최근 항목이 우선한다", () => {
    const older = item({ scope: "task", id: "old", title: "old login ticket", updatedAt: NOW - 6 * DAY });
    const newer = item({ scope: "task", id: "new", title: "new login ticket", updatedAt: NOW - 1 * DAY });
    const ranked = mergeQuickOpenResults("login", [[older, newer]], { now: NOW });
    expect(ranked.map((r) => r.id)).toEqual(["new", "old"]);
  });

  it("여러 소스 배열을 하나로 병합해 점수순 정렬한다", () => {
    const tasks: QuickOpenItem[] = [item({ scope: "task", id: "t1", title: "release train" })];
    const files: QuickOpenItem[] = [item({ scope: "file", id: "src/train.ts", title: "src/train.ts" })];
    const ranked = mergeQuickOpenResults("train", [tasks, files], { now: NOW });
    expect(ranked.map((r) => r.id).sort()).toEqual(["src/train.ts", "t1"]);
  });

  it("상한(기본 50건)을 넘는 결과는 잘라낸다", () => {
    const many: QuickOpenItem[] = Array.from({ length: 80 }, (_, i) =>
      item({ scope: "file", id: `f${i}`, title: `file-${i}.ts` }),
    );
    const ranked = mergeQuickOpenResults("file", [many], { now: NOW });
    expect(ranked).toHaveLength(50);
  });

  it("limit 옵션으로 상한을 조절할 수 있다", () => {
    const many: QuickOpenItem[] = Array.from({ length: 10 }, (_, i) =>
      item({ scope: "file", id: `f${i}`, title: `file-${i}.ts` }),
    );
    const ranked = mergeQuickOpenResults("file", [many], { now: NOW, limit: 3 });
    expect(ranked).toHaveLength(3);
  });

  it("scopes 옵션을 지정하면 해당 스코프만 포함한다", () => {
    const sources: QuickOpenItem[][] = [
      [item({ scope: "task", id: "t1", title: "alpha task" })],
      [item({ scope: "file", id: "src/alpha.ts", title: "src/alpha.ts" })],
      [item({ scope: "command", id: "cmd:alpha", title: "alpha command" })],
    ];
    const ranked = mergeQuickOpenResults("alpha", sources, { now: NOW, scopes: ["file"] });
    expect(ranked).toHaveLength(1);
    expect(ranked[0].scope).toBe("file");
  });

  it("매치되지 않는 항목은 결과에서 제외된다", () => {
    const sources: QuickOpenItem[][] = [
      [item({ scope: "task", id: "match", title: "release notes" })],
      [item({ scope: "task", id: "nomatch", title: "unrelated entry" })],
    ];
    const ranked = mergeQuickOpenResults("release", sources, { now: NOW });
    expect(ranked.map((r) => r.id)).toEqual(["match"]);
  });
});

describe("fileQuickOpenItems", () => {
  it("파일 경로 목록을 file 스코프 항목으로 변환한다", () => {
    const items = fileQuickOpenItems(["src/App.tsx", "src/lib/ipc.ts"]);
    expect(items).toEqual([
      { scope: "file", id: "src/App.tsx", title: "src/App.tsx" },
      { scope: "file", id: "src/lib/ipc.ts", title: "src/lib/ipc.ts" },
    ]);
  });
});

describe("QUICK_OPEN_COMMANDS", () => {
  it("새 작업/테마 전환/패널 토글/홈 정적 커맨드를 포함한다", () => {
    const actions = QUICK_OPEN_COMMANDS.map((c) => c.action);
    expect(actions).toEqual(
      expect.arrayContaining(["new-task", "toggle-theme", "toggle-activity", "go-home"]),
    );
    expect(QUICK_OPEN_COMMANDS.every((c) => c.scope === "command")).toBe(true);
  });
});

describe("mergeQuickOpenResults 동점 정렬", () => {
  it("빈 쿼리로 점수가 같은 파일은 제목의 로케일 순서로 정렬한다", () => {
    const titles = ["src/zeta.ts", "src/Alpha.ts", "src/beta.ts", "src/älpha.ts"];
    const ranked = mergeQuickOpenResults("", [fileQuickOpenItems(titles)], { now: NOW });
    expect(ranked.map((r) => r.title)).toEqual([...titles].sort((a, b) => a.localeCompare(b)));
  });
});
