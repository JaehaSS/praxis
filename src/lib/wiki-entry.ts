import type { WikiPage } from "./wiki-workspace-ipc";

/** 설정을 읽지 못했을 때 쓰는 진입 문서. Rust의 `DEFAULT_WIKI_HOME`과 같아야 한다. */
export const DEFAULT_WIKI_HOME = "위키-시작.md";

const nfc = (value: string) => value.normalize("NFC");
const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const depth = (path: string) => path.split("/").length;

/**
 * 위키를 열었을 때 먼저 띄울 문서를 고른다 — 창고를 고르는 것으로 끝내기 위한 진입점이다.
 *
 * 1. 설정한 경로와 그대로 맞는 문서.
 * 2. 같은 **파일 이름**을 가진 문서 중 가장 얕은 곳의 것. 위키 폴더가 `문서/기술-위키/wiki`처럼
 *    깊어도 `위키-시작.md` 한 줄로 가리킬 수 있어야 하기 때문이다.
 * 3. 그래도 없으면 역링크가 가장 많은 문서 — 링크가 모이는 곳이 사실상의 허브다.
 *
 * 셋 다 실패하는 경우는 문서가 하나도 없을 때뿐이고, 그때만 `null`이다. 고른 문서에서
 * 링크를 타고 들어가는 것이 기본 동선이지만, 링크가 끊긴 문서는 그렇게 닿을 수 없으므로
 * 목록과 검색은 이 선택과 무관하게 남는다.
 */
export function entryPage(pages: WikiPage[], home: string): string | null {
  if (!pages.length) return null;
  const wanted = nfc(home.trim()) || DEFAULT_WIKI_HOME;
  const exact = pages.find(page => page.id === wanted);
  if (exact) return exact.id;
  const name = fileName(wanted).toLocaleLowerCase();
  const named = pages
    .filter(page => fileName(page.id).toLocaleLowerCase() === name)
    .sort((a, b) => depth(a.id) - depth(b.id) || a.id.localeCompare(b.id));
  if (named.length) return named[0].id;
  const hub = [...pages].sort((a, b) => b.backlinks.length - a.backlinks.length || a.id.localeCompare(b.id));
  return hub[0].id;
}
