import { useHostScope } from "../lib/host-scope";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactElement,
} from "react";
import { type Memory, type MemoryVersion } from "../lib/ipc";
import {
  assertTransportSessionCurrent,
  captureTransportSession,
  getTransportRevision,
  subscribeTransportChange,
  type TransportSession,
} from "../lib/transport";
import {
  isVersionHistoryUnsupported,
  versionStateForMemory,
  type MemoryVersionLoadState,
} from "./memory-version";
import { useMemoryVersionRestoreAction } from "./memory-version-restore-action";
import { MemoryVersionComparison } from "./MemoryVersionComparison";
interface Props {
  memory: Memory;
  onChanged: () => Promise<void>;
}

interface StateViewProps {
  memory: Memory;
  state: MemoryVersionLoadState;
  onRetry: () => void;
  onRestore: (version: MemoryVersion) => Promise<void>;
  busy?: boolean;
}

interface OwnedVersionState {
  owner: string;
  state: MemoryVersionLoadState;
}

function visibleVersionState(
  owned: OwnedVersionState,
  owner: string,
  memoryId: number,
): MemoryVersionLoadState {
  if (owned.owner !== owner) return { memoryId, status: "loading" };
  return versionStateForMemory(owned.state, memoryId);
}

export function MemoryVersionStateView({
  memory,
  state,
  onRetry,
  onRestore,
  busy = false,
}: StateViewProps): ReactElement {
  if (state.status === "loading") {
    return <div className="text-text-muted">버전 이력을 불러오는 중…</div>;
  }
  if (state.status === "unsupported") {
    return <div className="text-text-muted">버전 이력 미지원 · Runner를 업데이트하세요.</div>;
  }
  if (state.status === "error") {
    return (
      <div className="rounded border border-status-failed/40 bg-status-failed/5 p-2">
        <div className="text-status-failed">{state.message}</div>
        <button className="mt-1 text-primary-bright" onClick={onRetry}>다시 시도</button>
      </div>
    );
  }
  return (
    <MemoryVersionComparison
      key={`${memory.id}:${memory.current_version}`}
      memory={memory}
      versions={state.versions}
      onRestore={onRestore}
      busy={busy}
    />
  );
}

function useVersionLoader(memoryId: number, revision: number): {
  state: MemoryVersionLoadState;
  reload: () => Promise<void>;
  session: TransportSession;
} {
  const requestRef = useRef(0);
  const ownerRef = useRef(`${memoryId}:${revision}`);
  // 메모리는 그것을 보관한 머신의 DB에 산다 — 스코프가 그 머신을 정한다 (ADR 0133).
  const host = useHostScope();
  const session = useMemo(
    () => captureTransportSession(host),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [host, memoryId, revision],
  );
  const [ownedState, setOwnedState] = useState<OwnedVersionState>({
    owner: ownerRef.current,
    state: { memoryId, status: "loading" },
  });
  const owner = `${memoryId}:${revision}`;
  const reload = useCallback(async (): Promise<void> => {
    if (ownerRef.current !== owner) return;
    const request = requestRef.current + 1;
    requestRef.current = request;
    setOwnedState({ owner, state: { memoryId, status: "loading" } });
    try {
      const versions = await session.transport.memoryVersions(memoryId);
      assertTransportSessionCurrent(session);
      if (requestRef.current === request) {
        setOwnedState({ owner, state: { memoryId, status: "ready", versions } });
      }
    } catch (reason) {
      if (requestRef.current !== request) return;
      if (isVersionHistoryUnsupported(reason)) {
        setOwnedState({ owner, state: { memoryId, status: "unsupported" } });
        return;
      }
      setOwnedState({
        owner,
        state: { memoryId, status: "error", message: String(reason) },
      });
    }
  }, [memoryId, owner, session]);
  useEffect(() => {
    ownerRef.current = owner;
    void reload();
    return () => {
      ownerRef.current = "";
      requestRef.current += 1;
    };
  }, [owner, reload]);
  const state = visibleVersionState(ownedState, owner, memoryId);
  return { state, reload, session };
}

export function MemoryVersionPanel({ memory, onChanged }: Props): ReactElement {
  const revision = useSyncExternalStore(
    subscribeTransportChange,
    getTransportRevision,
    getTransportRevision,
  );
  const { state, reload, session } = useVersionLoader(memory.id, revision);
  const owner = `${memory.id}:${revision}`;
  const action = useMemoryVersionRestoreAction({
    memory,
    owner,
    session,
    onChanged,
    reload,
  });
  return (
    <div className="mt-2 ml-7 border-t border-border pt-2 text-xs">
      <div className="mb-2 text-text-muted">버전 이력 · 선택한 과거 내용과 현재 비교</div>
      {action.error && (
        <div className="mb-1 text-status-failed">{action.error}</div>
      )}
      <MemoryVersionStateView
        memory={memory}
        state={state}
        onRetry={() => void reload()}
        onRestore={action.restore}
        busy={action.busy}
      />
    </div>
  );
}
