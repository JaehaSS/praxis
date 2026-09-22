import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { TaskRef } from "../lib/transport";
import type { FileDiff } from "../lib/ipc";
import {
  clearViewed,
  isViewed,
  loadViewed,
  markViewed,
  saveViewed,
  type ViewedState,
} from "../lib/diff-viewed";
import { useDiffReviewActions } from "./use-diff-review-actions";
import { useDiffViewerData, type DiffViewerData } from "./use-diff-viewer-data";
import { usePartialApply, type PartialApplyState } from "./use-partial-apply";

export type OpenDiff = (path: string, opts?: { preview?: boolean }) => void;

export interface DiffSessionValue {
  data: DiffViewerData;
  viewed: ViewedState;
  toggleViewed: (path: string) => void;
  /** 부분 적용은 선택 밖 hunk를 **모든 파일에서** 되돌린다 — 그래서 세션 값이다(ADR 0175 결정 8). */
  partial: PartialApplyState;
  actions: ReturnType<typeof useDiffReviewActions>;
  activeDiffPath: string | null;
  setActiveDiffPath: (path: string | null) => void;
  openDiff: OpenDiff;
  /** 폴링 구독 — 반환된 해제 함수를 부를 때까지 타이머가 돈다(설계 DR-7). */
  retain: () => () => void;
}

const DiffSession = createContext<DiffSessionValue | null>(null);

export function useDiffSession(): DiffSessionValue {
  const value = useContext(DiffSession);
  if (!value) throw new Error("useDiffSession은 DiffSessionProvider 안에서만 쓸 수 있다.");
  return value;
}

/** 구독자가 하나도 없으면 폴링을 멈춘다. 구독자는 변경 목록과 열린 diff 탭이다. */
function useSubscriberGate(): { enabled: boolean; retain: () => () => void } {
  const [count, setCount] = useState(0);
  const retain = useCallback(() => {
    setCount((n) => n + 1);
    return () => setCount((n) => n - 1);
  }, []);
  return { enabled: count > 0, retain };
}

/** 확인함은 호스트별로 갈라야 한다 — 같은 id의 다른 작업과 섞이면 안 된다. */
function useViewedFiles(taskId: number, files: FileDiff[] | null) {
  const [viewed, setViewed] = useState<ViewedState>(() => loadViewed(taskId));
  const filesRef = useRef(files);
  filesRef.current = files;
  useEffect(() => setViewed(loadViewed(taskId)), [taskId]);

  const toggleViewed = useCallback(
    (path: string) => {
      const file = filesRef.current?.find((f) => f.path === path);
      if (!file) return;
      setViewed((current) => {
        const next = isViewed(current, path, file.patch)
          ? clearViewed(current, path)
          : markViewed(current, path, file.patch);
        saveViewed(taskId, next);
        return next;
      });
    },
    [taskId],
  );
  return { viewed, toggleViewed };
}

/**
 * 세션당 하나뿐인 diff 상태. 변경 목록과 diff 탭들이 같은 스냅샷·같은 범위·같은 선택을 본다.
 *
 * 훅이 둘이면 폴링이 두 배이고, 범위가 어긋나는 순간 `hunk_id`가 달라져 붙여둔 주석이
 * 한꺼번에 고아로 보인다(설계 DR-7).
 */
export function DiffSessionProvider({
  task,
  openDiff,
  children,
}: {
  task: TaskRef;
  openDiff: OpenDiff;
  children: ReactNode;
}) {
  const { enabled, retain } = useSubscriberGate();
  const data = useDiffViewerData(task, enabled);
  const { viewed, toggleViewed } = useViewedFiles(task.id, data.files);
  const partial = usePartialApply(task, data.hunks, data.refresh);
  const [activeDiffPath, setActiveDiffPath] = useState<string | null>(null);
  const actions = useDiffReviewActions(task, activeDiffPath, data.annotations, data.refreshAnnotations);

  return (
    <DiffSession.Provider
      value={{
        data,
        viewed,
        toggleViewed,
        partial,
        actions,
        activeDiffPath,
        setActiveDiffPath,
        openDiff,
        retain,
      }}
    >
      {children}
    </DiffSession.Provider>
  );
}

/** 작업이 선택되지 않았으면 diff 세션도 없다. 작업이 바뀌면 통째로 다시 만든다(설계 F-5). */
export function DiffSessionScope({
  task,
  openDiff,
  children,
}: {
  task: TaskRef | null;
  openDiff: OpenDiff;
  children: ReactNode;
}) {
  if (!task) return <>{children}</>;
  return (
    <DiffSessionProvider key={`${task.host}:${task.id}`} task={task} openDiff={openDiff}>
      {children}
    </DiffSessionProvider>
  );
}
