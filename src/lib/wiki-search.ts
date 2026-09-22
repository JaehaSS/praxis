// src/lib/wiki-search.ts — 위키 패널 검색의 순위와 일치 조각.
//
// 서버 색인(`knowledge_vault_search` → vault_fts)을 쓰지 않는다. 그래프가 이미 모든 본문을
// 메모리에 싣고 있어 "더 많이 찾는" 이득이 없는 반면, 목록의 출처(그래프)와 검색의 출처(색인)가
// 갈리면 색인이 뒤처진 순간 클릭했는데 없는 문서로 가는 결함이 생긴다. 게다가 그쪽 검색에는
// scope 게이트가 있어서(`vault/index.rs`), 목록에는 보이는데 검색에는 안 나오는 문서가 생긴다.
// 창고가 메모리에 다 들어가지 않게 되면 그때 바꾼다.

/** 어느 필드에서 맞았나. 배열 순서가 곧 순위다. */
export type WikiMatchField = "title" | "alias" | "path" | "body";

const FIELD_RANK: Record<WikiMatchField, number> = { title: 0, alias: 1, path: 2, body: 3 };

/** 본문 스니펫에서 일치 앞뒤로 남길 글자 수. 뒤를 더 길게 두는 이유는 읽는 방향이 그쪽이기 때문이다. */
const SNIPPET_BEFORE = 24;
const SNIPPET_AFTER = 56;

/**
 * 일치 지점을 세 조각으로 쪼갠 것. 화면은 `before` + 강조한 `text` + `after`를 이어 그리면 된다.
 *
 * 오프셋 대신 조각을 돌려주는 이유는 공백 접기다. 스니펫은 줄바꿈을 한 칸으로 눌러야 목록에
 * 한 줄로 들어가는데, 누르고 나면 오프셋이 어긋난다. 조각을 각각 눌러 넘기면 호출부에 인덱스
 * 계산이 남지 않는다.
 */
export interface WikiMatch {
  field: WikiMatchField;
  before: string;
  /** 질의와 맞은 원문 조각. 대소문자는 접기 전 원문 그대로다. */
  text: string;
  after: string;
}

export interface SearchablePage {
  id: string;
  title: string;
  path: string;
  aliases: string[];
  body: string;
}

export interface WikiSearchResult<T> {
  page: T;
  /** 질의가 비었으면 null — 거르지 않은 목록이라는 뜻이다. */
  match: WikiMatch | null;
}

interface IndexField {
  field: WikiMatchField;
  /** NFC로 맞춘 원문. `folded`와 오프셋이 일치해야 하므로 여기서도 정규화한다. */
  source: string;
  folded: string;
}

export type WikiSearchIndex = Map<string, IndexField[]>;

/**
 * 오프셋을 보존하는 소문자 접기. 입력은 이미 NFC여야 한다.
 *
 * `toLocaleLowerCase()`는 대부분 길이를 유지하지만 İ(U+0130)처럼 늘어나는 글자가 있다.
 * 소문자 매핑에 줄어드는 경우는 없으므로 **전체 길이가 같다면 늘어난 글자도 없다**는 뜻이고,
 * 접은 문자열의 오프셋을 원문에 그대로 쓸 수 있다. 길이가 달라졌을 때만 글자 단위로 다시 접어
 * 늘어나는 글자를 원문으로 남긴다 — 느리지만 그런 문서는 드물다.
 */
function fold(source: string): string {
  const lower = source.toLocaleLowerCase();
  if (lower.length === source.length) return lower;
  return [...source]
    .map(char => {
      const one = char.toLocaleLowerCase();
      return one.length === char.length ? one : char;
    })
    .join("");
}

function indexField(field: WikiMatchField, value: string): IndexField {
  const source = value.normalize("NFC");
  return { field, source, folded: fold(source) };
}

/** 줄바꿈과 연속 공백을 한 칸으로. 목록 한 줄에 들어가게 하려는 것이다. */
function collapse(value: string): string {
  return value.replace(/\s+/g, " ");
}

/** 서러게이트 쌍 한가운데를 자르면 홀로 남은 반쪽이 깨진 글자로 그려진다 — 경계를 바깥으로 민다. */
function safeStart(text: string, index: number): number {
  if (index <= 0) return 0;
  const code = text.charCodeAt(index);
  return code >= 0xdc00 && code <= 0xdfff ? index - 1 : index;
}

function safeEnd(text: string, index: number): number {
  if (index >= text.length) return text.length;
  const code = text.charCodeAt(index);
  return code >= 0xdc00 && code <= 0xdfff ? index + 1 : index;
}

function matchField(entry: IndexField, needle: string): { match: WikiMatch; at: number } | null {
  const at = entry.folded.indexOf(needle);
  if (at < 0) return null;
  const end = at + needle.length;
  const text = collapse(entry.source.slice(at, end));
  if (entry.field !== "body") {
    return {
      at,
      match: {
        field: entry.field,
        before: collapse(entry.source.slice(0, at)),
        text,
        after: collapse(entry.source.slice(end)),
      },
    };
  }
  const from = safeStart(entry.source, Math.max(0, at - SNIPPET_BEFORE));
  const to = safeEnd(entry.source, Math.min(entry.source.length, end + SNIPPET_AFTER));
  return {
    at,
    match: {
      field: "body",
      before: (from > 0 ? "…" : "") + collapse(entry.source.slice(from, at)),
      text,
      after: collapse(entry.source.slice(end, to)) + (to < entry.source.length ? "…" : ""),
    },
  };
}

/**
 * 본문 접기를 한 번만 하려고 색인을 따로 만든다. 문서 집합이 그대로면 타이핑마다 다시 접지 않는다.
 *
 * 필드는 순위 순서로 넣는다 — 제목, 별칭, 경로, 본문. 먼저 맞는 필드에서 멈추므로 배열 순서가
 * 곧 "어느 일치를 보여줄 것인가"의 답이다.
 */
export function buildWikiSearchIndex(pages: readonly SearchablePage[]): WikiSearchIndex {
  const index: WikiSearchIndex = new Map();
  for (const page of pages) {
    index.set(page.id, [
      indexField("title", page.title),
      ...page.aliases.map(alias => indexField("alias", alias)),
      indexField("path", page.path),
      indexField("body", page.body),
    ]);
  }
  return index;
}

/**
 * 질의에 맞는 문서를 순위대로. 질의가 비면 받은 순서 그대로 전부 돌려준다.
 *
 * 필드가 1순위, 같은 필드 안에서는 이른 위치가 먼저다. 둘 다 같으면 받은 순서를 유지한다
 * (`Array.prototype.sort`는 안정 정렬이다) — 같은 질의에 같은 창고면 늘 같은 목록이 나온다.
 *
 * 필드를 이어 붙여 한 번에 찾던 예전 방식과 달리 필드마다 따로 본다. 경계를 걸친 일치
 * (제목 끝 + 경로 앞)는 뜻이 없는 우연이었으므로 함께 사라진다.
 */
export function searchWikiPages<T extends SearchablePage>(
  index: WikiSearchIndex,
  pages: readonly T[],
  query: string,
): WikiSearchResult<T>[] {
  const needle = fold(query.trim().normalize("NFC"));
  if (!needle) return pages.map(page => ({ page, match: null }));
  const found: { page: T; match: WikiMatch; at: number }[] = [];
  for (const page of pages) {
    // 색인은 호출부가 같은 문서 집합으로 만든다. 없는 id는 이미 사라진 문서이므로 거른다.
    for (const entry of index.get(page.id) ?? []) {
      const hit = matchField(entry, needle);
      if (hit) {
        found.push({ page, match: hit.match, at: hit.at });
        break;
      }
    }
  }
  found.sort((a, b) => FIELD_RANK[a.match.field] - FIELD_RANK[b.match.field] || a.at - b.at);
  return found.map(({ page, match }) => ({ page, match }));
}
