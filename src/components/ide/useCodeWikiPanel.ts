import { useCallback, useEffect, useRef, useState } from "react";

import type { CodeWikiStatus } from "../../lib/code-wiki-ipc";
import type { CodeWikiActions } from "./useCodeGraphPanel";

interface Options {
  actions?: CodeWikiActions;
  scope: number | null;
  sourcePath: string | null;
  dirty: boolean;
  onGenerated?: (status: CodeWikiStatus) => void;
}

function useRequestState() {
  const [status, setStatus] = useState<CodeWikiStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const scopeRef = useRef(0);
  const runningScopeRef = useRef<number | null>(null);
  const run = useCallback(async <T,>(request: () => Promise<T>, apply?: (value: T) => void) => {
    const requestScope = scopeRef.current;
    if (runningScopeRef.current === requestScope) return;
    runningScopeRef.current = requestScope;
    setBusy(true);
    setError(null);
    try {
      const value = await request();
      if (scopeRef.current === requestScope) apply?.(value);
    } catch (cause) {
      if (scopeRef.current === requestScope) setError(String(cause));
    } finally {
      if (runningScopeRef.current === requestScope) runningScopeRef.current = null;
      if (scopeRef.current === requestScope) setBusy(false);
    }
  }, []);
  return { status, error, busy, scopeRef, run, setStatus, setError };
}

export function useCodeWikiPanel({ actions, scope, sourcePath, dirty, onGenerated }: Options) {
  const [open, setOpen] = useState(false);
  const { status, error, busy, scopeRef, run, setStatus, setError } = useRequestState();
  const actionsRef = useRef(actions);
  actionsRef.current = actions;
  const generatedRef = useRef(onGenerated);
  generatedRef.current = onGenerated;

  const reload = useCallback(async () => {
    const current = actionsRef.current;
    if (current) await run(current.status, setStatus);
  }, [run, setStatus]);

  useEffect(() => {
    scopeRef.current += 1;
    setStatus(null);
    setError(null);
    setOpen(false);
    return () => { scopeRef.current += 1; };
  }, [actions, scope, sourcePath, scopeRef, setError, setStatus]);

  const generate = useCallback(async (path: string | null) => {
    const current = actionsRef.current;
    if (!current || dirty || status?.graphState !== "ready") return;
    await run(() => current.generate(path), (next) => {
      setStatus(next);
      generatedRef.current?.(next);
    });
  }, [dirty, run, setStatus, status?.graphState]);

  const openPath = useCallback(async (path: string) => {
    const current = actionsRef.current;
    if (current) await run(() => current.openPath(path));
  }, [run]);

  return {
    status,
    error,
    busy,
    open,
    generate,
    openPath,
    reload,
    show: () => { setOpen(true); void reload(); },
    close: () => setOpen(false),
  };
}
