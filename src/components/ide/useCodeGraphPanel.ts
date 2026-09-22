import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";

import type {
  CodeGraphImpact,
  CodeGraphDirection,
  CodeGraphNeighborhood,
  CodeGraphNeighborhoodNode,
  CodeGraphReport,
  CodeGraphStatus,
  ImpactedSymbol,
} from "../../lib/ipc";
import type { CodeWikiStatus } from "../../lib/code-wiki-ipc";

export interface CodeWikiActions {
  status: () => Promise<CodeWikiStatus>;
  generate: (sourcePath: string | null) => Promise<CodeWikiStatus>;
  openPath: (path: string) => Promise<void>;
}

export interface EditorCodeGraphActions {
  scope: number | null;
  /** Wiki 생성은 로컬 Rust 그래프에서만 제공한다. */
  wiki?: CodeWikiActions;
  status: () => Promise<CodeGraphStatus>;
  index: () => Promise<CodeGraphReport>;
  cancel: () => Promise<void>;
  impactAt: (request: {
    path: string;
    line: number;
    column: number;
    depth: number;
  }) => Promise<CodeGraphImpact>;
  neighborhoodAt: (request: {
    path: string;
    line: number;
    column: number;
    direction: CodeGraphDirection;
    depth: number;
  }) => Promise<CodeGraphNeighborhood>;
  openItem: (item: ImpactedSymbol) => Promise<void>;
  openNode: (node: CodeGraphNeighborhoodNode) => Promise<void>;
}

export interface CodeGraphSource {
  path: string;
  dirty: boolean;
  line: number;
  column: number;
  scrollTop?: number;
  scrollLeft?: number;
}

interface Options {
  actions?: EditorCodeGraphActions;
  path: string | null;
  dirty: boolean;
  getPosition: () => { line: number; column: number } | null;
}

interface PanelState {
  status: CodeGraphStatus | null;
  impact: CodeGraphImpact | null;
  neighborhood: CodeGraphNeighborhood | null;
  error: string | null;
  busy: boolean;
  open: boolean;
  setImpact: Dispatch<SetStateAction<CodeGraphImpact | null>>;
  setNeighborhood: Dispatch<SetStateAction<CodeGraphNeighborhood | null>>;
  setError: Dispatch<SetStateAction<string | null>>;
  setBusy: Dispatch<SetStateAction<boolean>>;
  setOpen: Dispatch<SetStateAction<boolean>>;
  reload: () => Promise<void>;
  scopeEpoch: () => number;
  scopeCurrent: (epoch: number) => boolean;
  queryRequest: () => number;
  queryCurrent: (request: number) => boolean;
}

function usePanelState(
  actions: EditorCodeGraphActions | undefined,
): PanelState {
  const [status, setStatus] = useState<CodeGraphStatus | null>(null);
  const [impact, setImpact] = useState<CodeGraphImpact | null>(null);
  const [neighborhood, setNeighborhood] =
    useState<CodeGraphNeighborhood | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const scopeEpochRef = useRef(0);
  const statusRequestRef = useRef(0);
  const queryRequestRef = useRef(0);
  const actionsRef = useRef(actions);
  if (actionsRef.current !== actions) {
    actionsRef.current = actions;
    scopeEpochRef.current += 1;
    statusRequestRef.current += 1;
    queryRequestRef.current += 1;
  }

  const reload = useCallback(async () => {
    const scopeEpoch = scopeEpochRef.current;
    const request = ++statusRequestRef.current;
    if (!actions) return;
    try {
      const status = await actions.status();
      if (
        scopeEpoch === scopeEpochRef.current &&
        request === statusRequestRef.current
      )
        setStatus(status);
    } catch (cause) {
      if (
        scopeEpoch === scopeEpochRef.current &&
        request === statusRequestRef.current
      )
        setError(String(cause));
    }
  }, [actions]);

  useEffect(() => {
    scopeEpochRef.current += 1;
    statusRequestRef.current += 1;
    queryRequestRef.current += 1;
    setStatus(null);
    setImpact(null);
    setNeighborhood(null);
    setError(null);
    setBusy(false);
    setOpen(false);
    void reload();
    return () => {
      scopeEpochRef.current += 1;
      statusRequestRef.current += 1;
      queryRequestRef.current += 1;
    };
  }, [actions, reload]);

  const polling = busy || [
    "indexing_symbols",
    "waiting_semantic",
    "indexing_edges",
  ].includes(status?.buildState ?? "");
  useEffect(() => {
    if (!polling) return;
    const timer = globalThis.setInterval(() => void reload(), 750);
    return () => globalThis.clearInterval(timer);
  }, [polling, reload]);

  return {
    status,
    impact,
    neighborhood,
    error,
    busy,
    open,
    setImpact,
    setNeighborhood,
    setError,
    setBusy,
    setOpen,
    reload,
    scopeEpoch: () => scopeEpochRef.current,
    scopeCurrent: (epoch) => epoch === scopeEpochRef.current,
    queryRequest: () => ++queryRequestRef.current,
    queryCurrent: (request) => request === queryRequestRef.current,
  };
}

function useIndexAction(
  actions: EditorCodeGraphActions | undefined,
  state: PanelState,
) {
  return useCallback(async () => {
    if (!actions) return;
    const scopeEpoch = state.scopeEpoch();
    state.setBusy(true);
    state.setError(null);
    state.setImpact(null);
    try {
      await actions.index();
    } catch (cause) {
      const message = String(cause);
      if (state.scopeCurrent(scopeEpoch) && !message.includes("취소")) {
        state.setError(message);
        state.setOpen(true);
      }
    } finally {
      if (state.scopeCurrent(scopeEpoch)) {
        state.setBusy(false);
        await state.reload();
      }
    }
  }, [actions, state]);
}

function useCancelAction(
  actions: EditorCodeGraphActions | undefined,
  state: PanelState,
) {
  return useCallback(async () => {
    if (!actions) return;
    const scopeEpoch = state.scopeEpoch();
    try {
      await actions.cancel();
    } catch (cause) {
      if (state.scopeCurrent(scopeEpoch)) {
        state.setError(String(cause));
        state.setOpen(true);
      }
    }
    if (state.scopeCurrent(scopeEpoch)) await state.reload();
  }, [actions, state]);
}

function currentSource(options: Options): CodeGraphSource | null {
  const position = options.getPosition();
  if (!options.path || !position) return null;
  return { path: options.path, dirty: options.dirty, ...position };
}

function useInspectAction(
  options: Options,
  state: PanelState,
  invalidated: MutableRefObject<boolean>,
) {
  return useCallback(
    async (source?: CodeGraphSource | null) => {
      const anchor = source ?? currentSource(options);
      if (!options.actions || !anchor) return;
      if (anchor.dirty || (source == null && invalidated.current)) {
        state.setError("저장 후 그래프 새로 고침이 필요합니다");
        return;
      }
      invalidated.current = false;
      const scopeEpoch = state.scopeEpoch();
      const request = state.queryRequest();
      state.setError(null);
      state.setOpen(true);
      try {
        const impact = await options.actions.impactAt({
          path: anchor.path,
          line: anchor.line,
          column: anchor.column,
          depth: 2,
        });
        if (state.scopeCurrent(scopeEpoch) && state.queryCurrent(request))
          state.setImpact(impact);
      } catch (cause) {
        if (state.scopeCurrent(scopeEpoch) && state.queryCurrent(request)) {
          state.setImpact(null);
          state.setError(String(cause));
        }
      }
    },
    [options, state, invalidated],
  );
}

function useNeighborhoodAction(options: Options, state: PanelState, invalidated: MutableRefObject<boolean>) {
  const anchorRef = useRef<CodeGraphSource | null>(null);
  useEffect(() => {
    anchorRef.current = null;
  }, [options.actions]);
  return useCallback(
    async (
      direction: CodeGraphDirection,
      depth: number,
      source?: CodeGraphSource,
    ) => {
      const anchor = source ?? anchorRef.current ?? currentSource(options);
      if (!options.actions || !anchor) return;
      if (anchor.dirty || (source == null && invalidated.current)) {
        state.setError("저장 후 그래프 새로 고침이 필요합니다");
        return;
      }
      invalidated.current = false;
      anchorRef.current = anchor;
      const scopeEpoch = state.scopeEpoch();
      const request = state.queryRequest();
      state.setError(null);
      try {
        const neighborhood = await options.actions.neighborhoodAt({
          path: anchor.path,
          line: anchor.line,
          column: anchor.column,
          direction,
          depth,
        });
        if (state.scopeCurrent(scopeEpoch) && state.queryCurrent(request))
          state.setNeighborhood(neighborhood);
      } catch (cause) {
        if (state.scopeCurrent(scopeEpoch) && state.queryCurrent(request)) {
          state.setNeighborhood(null);
          state.setError(String(cause));
        }
      }
    },
    [options, state, invalidated],
  );
}

export interface CodeGraphPanelState {
  status: CodeGraphStatus | null;
  impact: CodeGraphImpact | null;
  neighborhood: CodeGraphNeighborhood | null;
  error: string | null;
  busy: boolean;
  open: boolean;
  index: () => Promise<void>;
  cancel: () => Promise<void>;
  inspect: (source?: CodeGraphSource | null) => Promise<void>;
  inspectNeighborhood: (
    direction: CodeGraphDirection,
    depth: number,
    source?: CodeGraphSource,
  ) => Promise<void>;
  close: () => void;
  invalidate: () => void;
}

export function useCodeGraphPanel(
  options: Options,
): CodeGraphPanelState & {
  openItem: (item: ImpactedSymbol) => Promise<void> | undefined;
  openNode: (node: CodeGraphNeighborhoodNode) => Promise<void> | undefined;
} {
  const state = usePanelState(options.actions);
  const sourceInvalidated = useRef(false);
  const index = useIndexAction(options.actions, state);
  const cancel = useCancelAction(options.actions, state);
  const inspect = useInspectAction(options, state, sourceInvalidated);
  const inspectNeighborhood = useNeighborhoodAction(options, state, sourceInvalidated);

  return {
    status: state.status,
    impact: state.impact,
    neighborhood: state.neighborhood,
    error: state.error,
    busy: state.busy,
    open: state.open,
    index,
    cancel,
    inspect,
    inspectNeighborhood,
    close: () => state.setOpen(false),
    invalidate: () => {
      sourceInvalidated.current = true;
      state.queryRequest();
      state.setError("소스가 변경되었습니다. 다시 조회하세요.");
    },
    openItem: (item: ImpactedSymbol) => options.actions?.openItem(item),
    openNode: (node: CodeGraphNeighborhoodNode) =>
      options.actions?.openNode(node),
  };
}
