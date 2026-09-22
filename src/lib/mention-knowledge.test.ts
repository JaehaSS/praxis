import { describe, it, expect } from "vitest";
import {
  mergeMentionItems,
  mentionInsertText,
  mentionKey,
  sourceLabel,
  type MentionItem,
} from "./mention-knowledge";
import type { KnowledgeHit } from "./ipc";

const hit = (overrides: Partial<KnowledgeHit> = {}): KnowledgeHit => ({
  chunk_id: 1,
  node_id: 1,
  source: "obsidian",
  title: "지식 그래프 설계",
  heading: "검색",
  url: "file:///vault/note.md",
  snippet: "본문 일부",
  score: 0.5,
  ...overrides,
});

describe("mergeMentionItems", () => {
  it("파일 결과가 항상 지식보다 앞에 온다", () => {
    // 파일 멘션은 기존 동작이다. 순서가 바뀌면 손이 기억하는 위치가 어긋난다.
    const merged = mergeMentionItems(["src/a.ts"], [hit()]);
    expect(merged[0]).toMatchObject({ kind: "file", path: "src/a.ts" });
    expect(merged[1]).toMatchObject({ kind: "knowledge" });
  });

  it("지식 결과가 없어도 파일 결과만으로 동작한다", () => {
    // 검색이 실패하거나 늦어도 파일 멘션은 종전대로여야 한다.
    expect(mergeMentionItems(["src/a.ts"], [])).toHaveLength(1);
  });

  it("파일이 없어도 지식만으로 목록이 열린다", () => {
    // repo 미선택 상태에서도 지식 멘션은 떠야 한다 (DR-13: 지식은 전역 스코프).
    const merged = mergeMentionItems([], [hit()]);
    expect(merged).toHaveLength(1);
    expect(merged[0].kind).toBe("knowledge");
  });

  it("둘 다 비면 빈 목록이다", () => {
    expect(mergeMentionItems([], [])).toEqual([]);
  });

  it("같은 청크가 두 번 오면 하나만 남는다", () => {
    const merged = mergeMentionItems([], [hit(), hit()]);
    expect(merged).toHaveLength(1);
  });

  it("전체 개수를 상한으로 자르되 파일을 먼저 지킨다", () => {
    // 지식 결과가 파일 결과를 목록 밖으로 밀어내면 기존 동작의 회귀다.
    const files = ["a.ts", "b.ts", "c.ts"];
    const hits = Array.from({ length: 20 }, (_, i) =>
      hit({ chunk_id: i + 10, title: `문서${i}` }),
    );
    const merged = mergeMentionItems(files, hits, 8);
    expect(merged).toHaveLength(8);
    expect(merged.slice(0, 3).every((m) => m.kind === "file")).toBe(true);
  });
});

describe("mentionInsertText", () => {
  it("파일 삽입 형식은 기존 그대로다", () => {
    // applyMention이 `@${text} `로 넣으므로 경로가 그대로여야 기존 계약이 유지된다.
    expect(mentionInsertText({ kind: "file", path: "src/a.ts" })).toBe("src/a.ts");
  });

  it("로컬 출처(Obsidian)는 절대 경로로 삽입된다", () => {
    // vault 노트는 진짜 파일이다. 경로만 주면 에이전트가 기존 @파일 멘션과 똑같이 읽는다 —
    // 본문을 따로 실어 나르는 배관이 필요 없다.
    const item: MentionItem = {
      kind: "knowledge",
      hit: hit({ url: "file:///Users/me/vault/노트.md" }),
    };
    expect(mentionInsertText(item)).toBe("/Users/me/vault/노트.md");
  });

  it("원격 출처는 읽을 수단이 없으므로 라벨로 삽입된다", () => {
    const item: MentionItem = {
      kind: "knowledge",
      hit: hit({ source: "notion", url: "https://notion.so/page" }),
    };
    expect(mentionInsertText(item)).toBe("[Notion: 지식 그래프 설계]");
  });

  it("제목의 대괄호는 라벨 경계를 깨지 않게 치환한다", () => {
    const item: MentionItem = {
      kind: "knowledge",
      hit: hit({ source: "gmail", url: null, title: "회의 [초안] 정리" }),
    };
    expect(mentionInsertText(item)).toBe("[Gmail: 회의 (초안) 정리]");
  });

  it("URL이 없어도 터지지 않는다", () => {
    const item: MentionItem = { kind: "knowledge", hit: hit({ url: null }) };
    expect(mentionInsertText(item)).toBe("[Obsidian: 지식 그래프 설계]");
  });
});

describe("sourceLabel", () => {
  it("소스별 표시 이름을 준다", () => {
    expect(sourceLabel("obsidian")).toBe("Obsidian");
    expect(sourceLabel("notion")).toBe("Notion");
    expect(sourceLabel("gmail")).toBe("Gmail");
  });

  it("모르는 소스는 원문을 그대로 보여준다", () => {
    // Phase 3·4에서 소스가 늘어난다. 여기서 조용히 빈 배지가 되면 안 된다.
    expect(sourceLabel("slack")).toBe("slack");
  });
});

describe("mentionKey", () => {
  it("파일과 지식이 서로 다른 키 공간을 쓴다", () => {
    const fileKey = mentionKey({ kind: "file", path: "1" });
    const knowledgeKey = mentionKey({ kind: "knowledge", hit: hit({ chunk_id: 1 }) });
    expect(fileKey).not.toBe(knowledgeKey);
  });
});
