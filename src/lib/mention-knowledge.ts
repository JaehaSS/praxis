// src/lib/mention-knowledge.ts — @멘션에 외부 지식 결과를 얹는 순수 로직.
//
// `mention.ts`는 건드리지 않는다. 저쪽은 순수 함수 4개에 전용 테스트가 있는 잘 격리된
// 계약이고, 여기에 비동기 소스와 출처 개념을 넣으면 순수성이 깨지면서 두 컴포저의
// 회귀 위험이 함께 커진다. 보존해야 할 것을 안 건드리는 게 가장 싼 보존이다 (설계 0020 DR-7).

import type { KnowledgeHit } from "./ipc";

export type MentionItem =
  | { kind: "file"; path: string }
  | { kind: "knowledge"; hit: KnowledgeHit };

const SOURCE_LABELS: Record<string, string> = {
  obsidian: "Obsidian",
  notion: "Notion",
  gmail: "Gmail",
};

/** 배지에 쓰는 소스 이름. 모르는 소스는 원문 그대로 — 조용히 빈 배지가 되지 않게. */
export function sourceLabel(source: string): string {
  return SOURCE_LABELS[source] ?? source;
}

/** React key. 파일 경로와 청크 id가 우연히 같아도 충돌하지 않게 접두사를 붙인다. */
export function mentionKey(item: MentionItem): string {
  return item.kind === "file" ? `f:${item.path}` : `k:${item.hit.chunk_id}`;
}

/**
 * 파일 결과와 지식 결과를 합친다.
 *
 * **파일이 언제나 먼저다.** 파일 멘션은 기존 동작이고, 순서가 바뀌면 손이 기억하는
 * 위치가 어긋난다. 상한을 넘을 때도 파일 쪽을 먼저 채워 지식 결과가 파일을 목록 밖으로
 * 밀어내지 못하게 한다.
 */
export function mergeMentionItems(
  files: string[],
  hits: KnowledgeHit[],
  limit = 8,
): MentionItem[] {
  const fileItems: MentionItem[] = files.map((path) => ({ kind: "file", path }));
  const seen = new Set<number>();
  const knowledgeItems: MentionItem[] = [];
  for (const hit of hits) {
    if (seen.has(hit.chunk_id)) continue;
    seen.add(hit.chunk_id);
    knowledgeItems.push({ kind: "knowledge", hit });
  }
  return [...fileItems, ...knowledgeItems].slice(0, limit);
}

/** `file://` URL을 로컬 절대 경로로. 로컬 출처가 아니면 null. */
export function localPathOf(url: string | null): string | null {
  if (!url?.startsWith("file://")) return null;
  try {
    return decodeURIComponent(url.slice("file://".length)) || null;
  } catch {
    return null;
  }
}

/**
 * `applyMention(value, caret, text)`에 넘길 문자열. 결과는 `@<text> `가 된다.
 *
 * - 파일: 경로 그대로 — 기존 계약이다.
 * - **로컬 출처(Obsidian): 절대 경로.** vault 노트는 진짜 파일이라, 경로만 주면 에이전트가
 *   기존 `@파일` 멘션과 똑같이 읽는다. 본문을 따로 실어 나르는 배관이 필요 없다.
 * - 원격 출처(Gmail·Notion): 읽을 수단이 없으므로 출처가 보이는 라벨만 넣는다.
 *   본문 첨부는 그 커넥터가 생기는 Phase 3·4에서 함께 만든다 — 지금 만들면 쓰는 곳 없이
 *   추측으로 설계하게 된다.
 */
export function mentionInsertText(item: MentionItem): string {
  if (item.kind === "file") return item.path;
  const local = localPathOf(item.hit.url);
  if (local) return local;
  // 제목의 대괄호는 라벨 경계를 깨뜨린다 — 나중에 라벨을 파싱할 여지를 남겨 둔다.
  const title = item.hit.title.replace(/\[/g, "(").replace(/\]/g, ")");
  return `[${sourceLabel(item.hit.source)}: ${title}]`;
}
