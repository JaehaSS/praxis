import { useEffect, useState } from "react";
import { observedModels } from "./ipc";
import type { ObservedModel } from "./models";

/**
 * 관측된 실행 모델 목록. 모델 드롭다운을 여는 두 화면(세션 선택·설정 기본값)이 공유한다.
 *
 * 실패하면 빈 배열로 둔다 — 이건 카탈로그를 **보강**하는 부가 정보라, 못 읽었다고 드롭다운
 * 자체를 막거나 오류를 띄울 이유가 없다. 원격 Runner 연결 중에도 같은 이유로 조용히 빈다.
 */
export function useObservedModels(): ObservedModel[] {
  const [models, setModels] = useState<ObservedModel[]>([]);
  useEffect(() => {
    let alive = true;
    observedModels()
      .then((rows) => {
        if (alive) setModels(rows);
      })
      .catch(() => {
        /* 카탈로그만으로 동작한다 */
      });
    return () => {
      alive = false;
    };
  }, []);
  return models;
}
