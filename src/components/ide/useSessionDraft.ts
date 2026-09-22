import { useCallback, useLayoutEffect, useRef, useState } from "react";

/**
 * 세션별로 작성 중이던 컴포저 입력을 기억한다.
 *
 * 컴포저는 화면에 하나뿐이라 입력값도 하나였고, 선택이 바뀔 때마다 비웠다. 그래서 A에 쓰다
 * B를 잠깐 보고 돌아오면 쓰던 문장이 사라졌다 — 잃는 것이 에이전트 출력이 아니라 **사용자가
 * 직접 친 글자**라 되찾을 경로가 없다. 세션별로 갈라 사는 상태(캡처 첨부·트랜스크립트 캐시)는
 * 이미 작업 단위로 나뉘어 있었고, 입력값만 공유 슬롯에 남아 있었다.
 *
 * 키는 `taskKey`(host:id) 좌표다. 로컬 3번과 원격 3번은 서로 다른 세션이라 숫자 하나로 묶으면
 * 서로의 초안을 덮는다(ADR 0133 결정 4). 좌표는 목록 조회와 무관하게 유지되므로, 새로고침이
 * 잠깐 그 작업을 목록에서 떨어뜨려도 초안은 흔들리지 않는다.
 *
 * 메모리에만 둔다 — 앱을 껐다 켜면 사라진다.
 */
export function useSessionDraft(
  sessionKey: string | null,
): [string, (value: string) => void, (key: string, submitted: string) => void] {
  const store = useRef(new Map<string, string>());
  const [draft, setDraft] = useState("");
  const currentKey = useRef(sessionKey);
  currentKey.current = sessionKey;

  // 전환과 같은 프레임에 되돌린다. useEffect로 미루면 새 세션 화면에 옛 세션의 문장이 한 번
  // 깜빡이고 지워진다.
  useLayoutEffect(() => {
    setDraft(sessionKey == null ? "" : store.current.get(sessionKey) ?? "");
  }, [sessionKey]);

  const write = useCallback(
    (value: string) => {
      if (currentKey.current === sessionKey) setDraft(value);
      if (sessionKey == null) return;
      const map = store.current;
      map.delete(sessionKey); // 재삽입으로 최근 사용 순서를 만든다
      // 빈 입력은 남기지 않는다 — 전송하고 비운 세션은 돌아와도 비어 있어야 한다.
      if (value !== "") map.set(sessionKey, value);
    },
    [sessionKey],
  );

  const clearSubmitted = useCallback((key: string, submitted: string) => {
    if ((store.current.get(key) ?? "") !== submitted) return;
    store.current.delete(key);
    if (currentKey.current === key) setDraft("");
  }, []);

  return [draft, write, clearSubmitted];
}
