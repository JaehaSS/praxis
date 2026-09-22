import { useEffect, useMemo, useState } from "react";
import { LOCAL_HOST } from "../../lib/transport";
import { taskRef } from "../../lib/ipc";
import {
  diffHunks,
  ensembleCompose,
  ensembleMatrix,
  type DiffHunk,
  type EnsembleMatrix,
  type HunkRef,
  type Task,
} from "../../lib/ipc";
import {
  composeSelections,
  conflictingSelections,
  defaultComposeSelection,
  summarizeComposeSelection,
  toggleComposeSelection,
} from "../../lib/ensemble-compose";

export interface EnsembleCompareState {
  hunksByTask: Record<number, DiffHunk[]>;
  allPaths: string[];
  file: string | null;
  setFile: (path: string) => void;
  activeId: number;
  setActiveId: (id: number) => void;
  selection: Set<string>;
  toggle: (ref: HunkRef) => void;
  notice: string | null;
  summary: Record<number, number>;
  composeArgs: HunkRef[];
  compose: () => Promise<void>;
  busy: boolean;
  loadError: string | null;
  appliedCount: number | null;
}

/** B-3 EnsembleCompare 상태 머신 — matrix/hunks 로드, 배타 그룹 인지 선택 토글, 조합 병합 호출.
 *  선택/토글 계산 자체는 `lib/ensemble-compose.ts` 순수 함수에 위임한다. */
export function useEnsembleCompare(
  ensemble: string,
  candidates: Task[],
  winnerTaskId: number,
  onComposed: () => void,
): EnsembleCompareState {
  const [hunksByTask, setHunksByTask] = useState<Record<number, DiffHunk[]>>({});
  const [matrix, setMatrix] = useState<EnsembleMatrix | null>(null);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [activeId, setActiveId] = useState(winnerTaskId);
  const [file, setFile] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [appliedCount, setAppliedCount] = useState<number | null>(null);

  // 후보는 같은 호스트에서 갈라진 형제들이다 — 첫 후보의 호스트가 곧 앙상블의 호스트다.
  const ensembleHost = candidates[0]?.host ?? LOCAL_HOST;
  const idsKey = candidates
    .map((c) => c.id)
    .sort((a, b) => a - b)
    .join(",");

  useEffect(() => {
    let cancelled = false;
    setLoadError(null);
    Promise.all([
      ensembleMatrix(ensembleHost, ensemble),
      Promise.all(candidates.map(async (c) => [c.id, await diffHunks(taskRef(c))] as const)),
    ])
      .then(([m, entries]) => {
        if (cancelled) return;
        const byTask = Object.fromEntries(entries);
        setMatrix(m);
        setHunksByTask(byTask);
        const winnerHunkIds = (byTask[winnerTaskId] ?? []).map((h: DiffHunk) => h.id);
        setSelection(defaultComposeSelection(winnerTaskId, winnerHunkIds));
        setActiveId(winnerTaskId);
        setAppliedCount(null);
      })
      .catch((e) => !cancelled && setLoadError(String(e)));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ensemble, winnerTaskId, idsKey]);

  const allPaths = useMemo(() => {
    const paths = new Set<string>();
    Object.values(hunksByTask).forEach((hs) => hs.forEach((h) => paths.add(h.path)));
    return Array.from(paths).sort();
  }, [hunksByTask]);

  useEffect(() => {
    if (file && allPaths.includes(file)) return;
    setFile(allPaths[0] ?? null);
  }, [allPaths, file]);

  const summary = summarizeComposeSelection(selection);
  const composeArgs = composeSelections(selection, winnerTaskId);

  const toggle = (ref: HunkRef) => {
    if (!matrix) return;
    const conflicts = conflictingSelections(selection, matrix, ref);
    setNotice(
      conflicts.length > 0
        ? `겹치는 선택 ${conflicts.length}건을 자동 해제했습니다 — 같은 영역은 하나만 선택할 수 있습니다.`
        : null,
    );
    setSelection((prev) => toggleComposeSelection(prev, matrix, ref));
  };

  const compose = async () => {
    setBusy(true);
    setLoadError(null);
    try {
      const outcome = await ensembleCompose(ensembleHost, ensemble, winnerTaskId, composeArgs);
      setAppliedCount(outcome.applied.length);
      onComposed();
    } catch (e) {
      setLoadError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return {
    hunksByTask,
    allPaths,
    file,
    setFile,
    activeId,
    setActiveId,
    selection,
    toggle,
    notice,
    summary,
    composeArgs,
    compose: () => compose(),
    busy,
    loadError,
    appliedCount,
  };
}
