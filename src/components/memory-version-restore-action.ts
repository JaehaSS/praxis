import { useRef, useState } from "react";
import type { Memory, MemoryVersion } from "../lib/ipc";
import {
  assertTransportSessionCurrent,
  type TransportSession,
} from "../lib/transport";
import { restoreMemoryVersion } from "./memory-version";

interface VersionRestoreActionArgs {
  memory: Memory;
  owner: string;
  session: TransportSession;
  onChanged: () => Promise<void>;
  reload: () => Promise<void>;
}

interface VersionRestoreAction {
  restore: (version: MemoryVersion) => Promise<void>;
  busy: boolean;
  error: string | null;
}

interface OwnedActionState {
  owner: string;
  busy: boolean;
  error: string | null;
}

export function useMemoryVersionRestoreAction({
  memory,
  owner,
  session,
  onChanged,
  reload,
}: VersionRestoreActionArgs): VersionRestoreAction {
  const generationRef = useRef(0);
  const [action, setAction] = useState<OwnedActionState>({
    owner,
    busy: false,
    error: null,
  });
  const visible = action.owner === owner
    ? action
    : { owner, busy: false, error: null };
  const restore = async (version: MemoryVersion): Promise<void> => {
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    setAction({ owner, busy: true, error: null });
    let error: string | null = null;
    try {
      assertTransportSessionCurrent(session);
      const changed = await restoreMemoryVersion(memory, version, {
        askConfirmation: (message) => window.confirm(message),
        restore: (...args) => session.transport.memoryRestoreVersion(...args),
      });
      if (!changed) return;
      assertTransportSessionCurrent(session);
      await onChanged();
      await reload();
    } catch (reason) {
      error = String(reason);
      await onChanged();
      await reload();
    } finally {
      if (generationRef.current === generation) {
        setAction({ owner, busy: false, error });
      }
    }
  };
  return { restore, busy: visible.busy, error: visible.error };
}
