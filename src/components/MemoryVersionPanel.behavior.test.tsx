// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Memory, MemoryVersion } from "../lib/ipc";
import { registerTransport, type PraxisTransport } from "../lib/transport";
import { tauriTransport } from "../lib/transport/tauri";

vi.mock("@monaco-editor/react", () => ({
  DiffEditor: ({ original, modified }: { original: string; modified: string }) => (
    <div data-original={original} data-modified={modified}>diff</div>
  ),
}));

import { MemoryVersionPanel } from "./MemoryVersionPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;

const memory: Memory = {
  id: 7,
  tier: "project",
  scope_key: "/repo",
  kind: "decision",
  content: "current",
  source_session: null,
  confidence: 0,
  usage_count: 0,
  last_used: null,
  created_at: 1,
  knowledge_type: "decision",
  status: "candidate",
  current_version: 2,
  utility_score: 0,
  review_due_at: null,
  verified_at: null,
  stale_at: null,
  archived_at: null,
  dormant: false,
};

function history(prefix: string): MemoryVersion[] {
  return [2, 1].map((version) => ({
    memory_id: 7,
    version,
    content: `${prefix} v${version}`,
    knowledge_type: "decision",
    scope_snapshot: "/repo",
    created_at: version,
    editor_kind: version === 2 ? "human_edit" : "candidate_intake",
    evidence_count: 0,
  }));
}

function remote(
  versions: PraxisTransport["memoryVersions"],
  restore: PraxisTransport["memoryRestoreVersion"],
): PraxisTransport {
  return {
    ...tauriTransport,
    kind: "remote",
    memoryVersions: versions,
    memoryRestoreVersion: restore,
  };
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
} {
  let resolve = (_value: T): void => undefined;
  let reject = (_reason: unknown): void => undefined;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

let root: Root | null = null;
let container: HTMLDivElement | null = null;

async function mount(onChanged = vi.fn(async () => undefined)): Promise<void> {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root?.render(<MemoryVersionPanel memory={memory} onChanged={onChanged} />);
  });
}

function findButton(label: string): HTMLButtonElement | undefined {
  return Array.from(container?.querySelectorAll("button") ?? [])
    .find((button) => button.textContent?.includes(label));
}

async function flush(action: () => void): Promise<void> {
  await act(async () => { action(); await Promise.resolve(); });
}

afterEach(async () => {
  vi.restoreAllMocks();
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

describe("MemoryVersionPanel behavior", () => {
  it("discards a late old-host response and restores only through the displayed host", async () => {
    const oldResponse = deferred<MemoryVersion[]>();
    const oldRestore = vi.fn(async () => 3);
    const newVersions = vi.fn(async () => history("new"));
    const newRestore = vi.fn(async () => 3);
    registerTransport(remote(() => oldResponse.promise, oldRestore));
    await mount();

    await act(async () => registerTransport(remote(newVersions, newRestore)));
    oldResponse.resolve(history("old"));
    await act(async () => Promise.resolve());

    expect(newVersions).toHaveBeenCalled();
    expect(container?.querySelector("[data-original]")?.getAttribute("data-original"))
      .toBe("new v1");
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const restoreButton = findButton("새 후보로 복원");
    await act(async () => restoreButton?.click());

    expect(oldRestore).not.toHaveBeenCalled();
    expect(newRestore).toHaveBeenCalledWith(7, 1, 2, "candidate");
  });

  it("reloads both the current memory and history after a restore conflict", async () => {
    const versions = vi.fn(async () => history("host"));
    const conflict = Object.assign(new Error("newer version"), { status: 409 });
    const restore = vi.fn(async () => Promise.reject(conflict));
    const onChanged = vi.fn(async () => undefined);
    registerTransport(remote(versions, restore));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await mount(onChanged);

    const restoreButton = findButton("새 후보로 복원");
    await act(async () => restoreButton?.click());

    expect(onChanged).toHaveBeenCalledOnce();
    expect(versions).toHaveBeenCalledTimes(2);
    expect(container?.textContent).toContain("newer version");
  });

  it("retries a failed history request instead of treating it as empty", async () => {
    const versions = vi.fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(history("retry"));
    registerTransport(remote(versions, vi.fn(async () => 3)));
    await mount();
    expect(container?.textContent).toContain("offline");

    const retry = findButton("다시 시도");
    await act(async () => retry?.click());

    expect(versions).toHaveBeenCalledTimes(2);
    expect(container?.querySelector("[data-original]")?.getAttribute("data-original"))
      .toBe("retry v1");
  });

  it("owns pending restore busy and errors by host revision", async () => {
    const pending = deferred<number>();
    const oldRestore = vi.fn(() => pending.promise);
    registerTransport(remote(vi.fn(async () => history("old")), oldRestore));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await mount();
    const oldButton = findButton("새 후보로 복원");

    await flush(() => oldButton?.click());
    oldButton?.click();
    expect(oldRestore).toHaveBeenCalledOnce();
    expect(oldButton?.disabled).toBe(true);

    const newPending = deferred<number>();
    const newRestore = vi.fn(() => newPending.promise);
    await act(async () =>
      registerTransport(remote(vi.fn(async () => history("new")), newRestore)));
    const newButton = findButton("새 후보로 복원");
    expect(newButton?.disabled).toBe(false);
    await flush(() => newButton?.click());
    expect(newRestore).toHaveBeenCalledOnce();

    await flush(() => pending.reject(new Error("old host failure")));
    const pendingNewButton = findButton("새 후보로 복원");
    expect(container?.textContent).not.toContain("old host failure");
    expect(pendingNewButton?.disabled).toBe(true);

    await flush(() => newPending.resolve(3));
    const settledNewButton = findButton("새 후보로 복원");
    expect(settledNewButton?.disabled).toBe(false);
  });
});
