import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { designmodeState, type NativePreviewState } from "../lib/ipc";

interface Options {
  selectedTaskId: number | null;
  selectTask: (taskId: number) => boolean;
  showPreview: () => void;
}

/** Inline의 실제 작업을 선택한 다음 패널을 연다. 이전 작업의 패널 설정은 건드리지 않는다. */
export function usePreviewActivation(options: Options): void {
  const latest = useRef(options);
  latest.current = options;
  const [pending, setPending] = useState<number | null>(null);
  useEffect(() => {
    let disposed = false;
    let revision = 0;
    const unlisten = listen<NativePreviewState>("designmode://activated", ({ payload }) => {
      const request = ++revision;
      void designmodeState(payload.taskId).then((preview) => {
        if (disposed || request !== revision || !preview || preview.generation !== payload.generation) return;
        if (preview.mode !== "inline") return;
        if (latest.current.selectTask(preview.taskId)) setPending(preview.taskId);
      }).catch(() => undefined);
    });
    return () => { disposed = true; void unlisten.then((off) => off()); };
  }, []);
  useEffect(() => {
    if (pending === null || pending !== options.selectedTaskId) return;
    latest.current.showPreview();
    setPending(null);
  }, [pending, options.selectedTaskId]);
}
