import { useEffect, useRef, useState } from "react";
import type { TaskRef } from "../lib/transport";
import { partialApply, partialRollback, type DiffHunk, type PartialApplyResult } from "../lib/ipc";
import { defaultPartialSelection, summarizePartialSelection, togglePartialSelection } from "../lib/partial";

export interface PartialApplyState {
  selection: Set<string>;
  toggle: (hunk: DiffHunk) => void;
  confirming: boolean;
  keptCount: number;
  discardedCount: number;
  requestConfirm: () => void;
  cancelConfirm: () => void;
  apply: () => Promise<void>;
  rollback: () => Promise<void>;
  busy: boolean;
  result: PartialApplyResult | null;
  error: string | null;
}

/** hunk 부분 승인(B-2) 상태 머신 — 선택 → 확인 → 적용/롤백. taskId 전환 시 결과 배너를
 *  초기화하고, hunks 갱신(초기 로드·적용 후 재조회) 시 선택만 새 diff 기준으로 재계산한다. */
export function usePartialApply(task: TaskRef, hunks: DiffHunk[], reload: () => void): PartialApplyState {
  const [selection, setSelection] = useState<Set<string>>(() => defaultPartialSelection(hunks));
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<PartialApplyResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  // 배열 참조가 아니라 hunk 구성이 바뀔 때만 재계산한다. Diff 화면은 5초마다 IPC에서
  // 새 배열을 받으므로, 참조를 의존성으로 두면 사용자가 해제한 hunk가 폴링마다 되살아난다.
  const hunkKey = hunks.map((hunk) => hunk.id).join(",");
  const hunksRef = useRef(hunks);
  hunksRef.current = hunks;

  useEffect(() => {
    setSelection(defaultPartialSelection(hunksRef.current));
    setConfirming(false);
  }, [hunkKey]);

  useEffect(() => {
    setResult(null);
    setError(null);
  }, [task.host, task.id]);

  const { keptIds, discardedIds } = summarizePartialSelection(hunks, selection);

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      const outcome = await partialApply(task, keptIds);
      setResult(outcome);
      setConfirming(false);
      reload();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const rollback = async () => {
    setBusy(true);
    setError(null);
    try {
      await partialRollback(task);
      setResult(null);
      reload();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return {
    selection,
    toggle: (hunk) => !busy && setSelection((prev) => togglePartialSelection(prev, hunk)),
    confirming,
    keptCount: keptIds.length,
    discardedCount: discardedIds.length,
    requestConfirm: () => setConfirming(true),
    cancelConfirm: () => setConfirming(false),
    apply,
    rollback,
    busy,
    result,
    error,
  };
}
