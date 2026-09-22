import { useEffect, useRef, useState } from "react";
import { knowledgeSearch, type KnowledgeHit } from "../lib/ipc";

/** 지식 검색 debounce. 타이핑 중 매 글자마다 IPC를 부르지 않게 한다. */
const DEBOUNCE_MS = 150;

/**
 * `@`토큰에 대한 외부 지식 검색.
 *
 * **파일 결과를 절대 기다리지 않는다.** 호출부는 파일 목록을 먼저 그리고, 여기서 온 결과를
 * 나중에 덧붙인다. 지식 검색이 늦거나 실패해도 파일 멘션은 종전과 같은 지연으로 뜬다 —
 * 이것이 "기존 동작 보존"의 실질이다 (설계 0020 DR-7).
 *
 * repo 스코프로 좁히지 않는다. Gmail·Notion·Obsidian은 프로젝트에 매여 있지 않고,
 * "이 repo에 관련된 지식"을 가리는 일은 스코프가 아니라 순위 문제다 (DR-13).
 */
export function useKnowledgeMentions(token: string | null): {
  hits: KnowledgeHit[];
  pending: boolean;
} {
  const [hits, setHits] = useState<KnowledgeHit[]>([]);
  const [pending, setPending] = useState(false);
  // 요청마다 증가시켜 늦게 도착한 응답(stale)을 버린다 — 컴포저의 기존 관례와 같다.
  const reqRef = useRef(0);

  useEffect(() => {
    if (token === null) {
      setHits([]);
      setPending(false);
      return;
    }
    const reqId = ++reqRef.current;
    setPending(true);
    const timer = setTimeout(() => {
      knowledgeSearch(token, 8)
        .then((found) => {
          if (reqId !== reqRef.current) return;
          setHits(found);
          setPending(false);
        })
        .catch(() => {
          // 지식 그래프가 비었거나 커맨드가 없어도 파일 멘션은 계속 동작해야 한다.
          if (reqId !== reqRef.current) return;
          setHits([]);
          setPending(false);
        });
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [token]);

  return { hits, pending };
}
