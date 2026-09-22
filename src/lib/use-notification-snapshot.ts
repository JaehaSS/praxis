import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { LOCAL_HOST, getTransport, listHosts, type HostId } from "./transport";
import { RunnerTransport } from "./transport/runner";
import {
  notificationIngest,
  notificationReconcile,
  notificationSnapshot,
  notificationSourcePage,
  type NotificationSnapshot,
  type SourceCursor,
  type SourcePage,
} from "./notifications";

export interface NotificationCollector {
  reconcile(host: HostId): void;
  errors: Record<string, string>;
}

interface Recovery {
  sourceId: string;
  watermark: number;
  recovered: boolean;
}

export function useNotificationSnapshot() {
  const [snapshot, setSnapshotState] = useState<NotificationSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const subscriptionFailed = useRef(false);
  const setSnapshot = useCallback((next: NotificationSnapshot) => {
    request.current += 1;
    setSnapshotState(next);
  }, []);
  const load = useCallback(async () => {
    const current = ++request.current;
    try {
      const next = await notificationSnapshot();
      if (current !== request.current) return;
      setSnapshotState(next);
      if (!subscriptionFailed.current) setError(null);
    } catch (reason) {
      if (current === request.current) setError(String(reason));
    }
  }, []);
  useEffect(() => {
    subscriptionFailed.current = false;
    let active = true;
    let stop: (() => void) | undefined;
    void load();
    void Promise.resolve()
      .then(() => listen("notification://changed", () => void load()))
      .then((unlisten) => {
        if (!active) return unlisten();
        stop = unlisten;
      })
      .catch((reason) => {
        if (!active) return;
        subscriptionFailed.current = true;
        setError(String(reason));
      });
    return () => {
      active = false;
      request.current += 1;
      if (stop) void Promise.resolve(stop()).catch(() => {});
    };
  }, [load]);
  return { snapshot, error, reload: load, setSnapshot, setError };
}

function cursorFor(sources: SourceCursor[], host: HostId): SourceCursor | undefined {
  return sources.find((source) => source.host === host);
}

function sameSnapshot(current: NotificationSnapshot | null, next: NotificationSnapshot): boolean {
  return current != null
    && current.enabled === next.enabled
    && current.delivery_error === next.delivery_error
    && JSON.stringify(current.items) === JSON.stringify(next.items)
    && JSON.stringify(current.sources) === JSON.stringify(next.sources);
}

function sourceFor(host: HostId) {
  return host === LOCAL_HOST
    ? notificationSourcePage
    : (after: number | null) => (getTransport(host) as RunnerTransport).notificationSourcePage(after);
}

export function useNotificationCollector(
  snapshot: NotificationSnapshot | null,
  setSnapshot: (snapshot: NotificationSnapshot) => void,
  setError: (error: string | null) => void,
): NotificationCollector {
  const inflight = useRef(new Set<HostId>());
  const recoveries = useRef(new Map<HostId, Recovery>());
  const transports = useRef(new Map<HostId, unknown>());
  const generation = useRef(0);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const sources = useRef<SourceCursor[]>(snapshot?.sources ?? []);
  const snapshotRef = useRef<NotificationSnapshot | null>(snapshot);
  const writes = useRef(new Map<HostId, Promise<void>>());
  const reconciling = useRef(new Set<HostId>());
  sources.current = snapshot?.sources ?? sources.current;
  snapshotRef.current = snapshot ?? snapshotRef.current;
  const save = useCallback((host: HostId, next: NotificationSnapshot) => {
    const current = snapshotRef.current;
    const merged = current == null ? next : {
      ...next,
      items: [...current.items.filter((item) => item.host !== host), ...next.items.filter((item) => item.host === host)],
      sources: [...current.sources.filter((source) => source.host !== host), ...next.sources.filter((source) => source.host === host)],
    };
    snapshotRef.current = merged;
    sources.current = merged.sources;
    if (sameSnapshot(current, merged)) return;
    setSnapshot(merged);
  }, [setSnapshot]);
  const write = useCallback(async <T,>(host: HostId, operation: () => Promise<T>): Promise<T> => {
    const previous = writes.current.get(host) ?? Promise.resolve();
    const next = previous.catch(() => {}).then(operation);
    writes.current.set(host, next.then(() => {}, () => {}));
    return next;
  }, []);
  const persist = useCallback(async (host: HostId, page: SourcePage, notify: boolean) => {
    return write(host, async () => {
      const result = await notificationIngest(host, page, notify);
      save(host, result);
      return result;
    });
  }, [save, write]);
  const clearError = useCallback((host: HostId) => {
    setErrors((previous) => {
      if (!(host in previous)) return previous;
      const { [host]: _, ...remaining } = previous;
      return remaining;
    });
  }, []);
  const collect = useCallback(async (host: HostId, currentGeneration: number) => {
    if (inflight.current.has(host)) return;
    inflight.current.add(host);
    try {
      const transport = host === LOCAL_HOST ? null : getTransport(host);
      if (transports.current.get(host) !== transport) recoveries.current.delete(host);
      transports.current.set(host, transport);
      const source = sourceFor(host);
      let known = cursorFor(sources.current, host);
      let page = await source(known?.cursor ?? null);
      if (currentGeneration !== generation.current) return;
      if (known && page.source_id !== known.source_id) page = await source(null);
      let recovery = recoveries.current.get(host);
      if (!recovery || recovery.sourceId !== page.source_id) {
        recovery = { sourceId: page.source_id, watermark: page.watermark, recovered: false };
        recoveries.current.set(host, recovery);
      }
      if (known && recovery.recovered && page.source_id === known.source_id
        && page.cursor === known.cursor && page.results.length === 0) {
        clearError(host);
        return;
      }
      while (true) {
        if (recovery.recovered) {
          await persist(host, page, true);
          break;
        }
        if (page.cursor > recovery.watermark) {
          const prefix = {
            ...page,
            cursor: recovery.watermark,
            watermark: recovery.watermark,
            results: page.results.filter((result) => result.sequence <= recovery!.watermark),
          };
          await persist(host, prefix, false);
          page = await source(recovery.watermark);
          if (page.source_id !== recovery.sourceId) throw new Error("알림 원천이 바뀌었습니다");
          await persist(host, page, true);
          recovery.recovered = true;
          break;
        }
        if (currentGeneration !== generation.current) return;
        await persist(host, page, false);
        if (page.cursor >= recovery.watermark) {
          recovery.recovered = true;
          break;
        }
        const after = page.cursor;
        page = await source(after);
        if (page.source_id !== recovery.sourceId || page.cursor <= after)
          throw new Error("알림 결과 페이지를 이어갈 수 없습니다");
      }
      clearError(host);
    } catch (reason) {
      recoveries.current.delete(host);
      setErrors((previous) => ({ ...previous, [host]: String(reason) }));
    } finally {
      inflight.current.delete(host);
    }
  }, [clearError, persist]);
  useEffect(() => {
    if (snapshot == null) return;
    const currentGeneration = ++generation.current;
    let active = true;
    const run = () => active && listHosts().forEach((host) => void collect(host, currentGeneration));
    run();
    const timer = window.setInterval(run, 750);
    return () => {
      active = false;
      generation.current += 1;
      window.clearInterval(timer);
    };
  }, [collect, snapshot !== null]);
  const reconcile = useCallback((host: HostId) => {
    if (reconciling.current.has(host)) return;
    reconciling.current.add(host);
    void write(host, async () => {
      const taskIds = (await getTransport(host).taskList()).map((task) => task.id);
      const result = await notificationReconcile(host, taskIds);
      save(host, result);
    }).catch((reason) => setError(`${host}: ${String(reason)}`)).finally(() => {
      reconciling.current.delete(host);
    });
  }, [save, setError, write]);
  return { reconcile, errors };
}
