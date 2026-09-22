// src/lib/quickopen.ts — Quick Open(⌘K) 랭킹 병합 순수 로직. UI/transport 비의존(테스트 용이성).
// 소스별 결과(작업/파일/세션/스킬/커맨드)를 최근성·접두 일치 가중으로 병합한다.

export type QuickOpenScope = "task" | "file" | "code" | "session" | "skill" | "command";

export interface QuickOpenItem {
  scope: QuickOpenScope;
  /** 스코프 내 고유 id — 파일=경로, 작업/세션=task id 문자열, 커맨드=액션 id. */
  id: string;
  title: string;
  subtitle?: string;
  /** 최근성 가중 기준 — epoch ms. 없으면 가중 0(정적 커맨드 등). */
  updatedAt?: number;
  /** 실행 액션 식별자 — 커맨드 스코프 전용, 선택 시 라우팅 키로 사용. */
  action?: QuickOpenCommandAction;
}

export interface RankedQuickOpenItem extends QuickOpenItem {
  score: number;
}

export interface MergeQuickOpenOptions {
  /** 반환 상한 (기본 50). */
  limit?: number;
  /** 지정 시 해당 스코프만 포함. 미지정=전체. */
  scopes?: QuickOpenScope[];
  /** 최근성 가중 기준 시각(테스트 결정성 주입용). 기본 Date.now(). */
  now?: number;
}

const RECENCY_WINDOW_MS = 7 * 24 * 60 * 60 * 1000; // 7일 — 이후엔 가중 0에 수렴
const RECENCY_WEIGHT = 10;
const EXACT_MATCH_WEIGHT = 100;
const PREFIX_MATCH_WEIGHT = 70;
const SUBSTRING_MATCH_WEIGHT = 40;
const SUBTITLE_MATCH_WEIGHT = 20;
const DEFAULT_LIMIT = 50;
/** 동점 정렬용 collator. `String.prototype.localeCompare`는 비교마다 collator를 다시 찾는다 —
 *  파일 수만 건이 전부 동점인 빈 쿼리에서 ICU 비교가 CPU 보고서의 핫스팟이었다(원장 #448). */
const TITLE_COLLATOR = new Intl.Collator();

function recencyScore(updatedAt: number | undefined, now: number): number {
  if (!updatedAt) return 0;
  const age = Math.max(0, now - updatedAt);
  if (age >= RECENCY_WINDOW_MS) return 0;
  return RECENCY_WEIGHT * (1 - age / RECENCY_WINDOW_MS);
}

/** 항목 하나의 매치 점수. 쿼리와 매치되지 않으면 null(제외 대상). */
export function scoreQuickOpenItem(item: QuickOpenItem, query: string, now: number): number | null {
  const q = query.trim().toLowerCase();
  const recency = recencyScore(item.updatedAt, now);
  if (!q) return recency;
  const title = item.title.toLowerCase();
  if (title === q) return EXACT_MATCH_WEIGHT + recency;
  if (title.startsWith(q)) return PREFIX_MATCH_WEIGHT + recency;
  if (title.includes(q)) return SUBSTRING_MATCH_WEIGHT + recency;
  if ((item.subtitle ?? "").toLowerCase().includes(q)) return SUBTITLE_MATCH_WEIGHT + recency;
  return null;
}

/** 소스별 결과 배열들을 점수 병합 → 스코프 필터 → 정렬 → 상한 슬라이스. */
export function mergeQuickOpenResults(
  query: string,
  sources: QuickOpenItem[][],
  opts: MergeQuickOpenOptions = {},
): RankedQuickOpenItem[] {
  const limit = opts.limit ?? DEFAULT_LIMIT;
  const now = opts.now ?? Date.now();
  const allowedScopes = opts.scopes ? new Set(opts.scopes) : null;
  const ranked: RankedQuickOpenItem[] = [];
  for (const items of sources) {
    for (const item of items) {
      if (allowedScopes && !allowedScopes.has(item.scope)) continue;
      const score = scoreQuickOpenItem(item, query, now);
      if (score == null) continue;
      ranked.push({ ...item, score });
    }
  }
  ranked.sort((a, b) => b.score - a.score || TITLE_COLLATOR.compare(a.title, b.title));
  return ranked.slice(0, limit);
}

/** 파일 경로 목록(flattenFiles 결과 재사용) → Quick Open file 스코프 항목. */
export function fileQuickOpenItems(paths: string[]): QuickOpenItem[] {
  return paths.map((path) => ({ scope: "file", id: path, title: path }));
}

/** 정적 커맨드 레지스트리 액션 — App.tsx가 선택 시 라우팅한다. */
export type QuickOpenCommandAction =
  | "new-task"
  | "toggle-theme"
  | "toggle-activity"
  | "toggle-code"
  | "go-home";

/** 정적 커맨드 레지스트리 — 새 작업/테마 전환/작업정보 토글/홈. */
export const QUICK_OPEN_COMMANDS: QuickOpenItem[] = [
  { scope: "command", id: "cmd:new-task", title: "새 작업", action: "new-task" },
  { scope: "command", id: "cmd:toggle-theme", title: "테마 전환", action: "toggle-theme" },
  { scope: "command", id: "cmd:toggle-activity", title: "작업 정보 토글", action: "toggle-activity" },
  { scope: "command", id: "cmd:toggle-code", title: "코드 열 토글", action: "toggle-code" },
  { scope: "command", id: "cmd:go-home", title: "홈으로", action: "go-home" },
];

/** `code` 항목이 실을 좌표 — 고르면 그 줄로 커서를 옮긴다. */
export interface CodeHit {
  path: string;
  line: number;
  column: number;
  text: string;
}

/**
 * 내용 검색 결과 → QuickOpen 항목.
 *
 * **랭킹을 매기지 않는다.** 백엔드가 준 순서(파일 경로순, 파일 안에서는 줄 번호순)가 곧
 * 사용자가 기대하는 순서다 — 관련도로 재정렬하면 한 글자 더 치는 동안 방금 눈으로 짚은
 * 행이 다른 데로 간다(BranchPicker와 같은 규칙).
 */
export function codeQuickOpenItems(hits: CodeHit[]): QuickOpenItem[] {
  return hits.map((h) => ({
    scope: "code" as const,
    id: `${h.path}:${h.line}:${h.column}`,
    title: `${h.path}:${h.line}`,
    subtitle: h.text.trim(),
  }));
}

/** `code` 항목 id에서 좌표를 되찾는다. 경로에 `:`가 있을 수 있으므로 **뒤에서** 자른다. */
export function parseCodeItemId(id: string): { path: string; line: number; column: number } | null {
  const m = id.match(/^(.*):(\d+):(\d+)$/);
  if (!m) return null;
  return { path: m[1], line: Number(m[2]), column: Number(m[3]) };
}
