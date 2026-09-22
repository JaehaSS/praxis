// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({
  partialApply: vi.fn(),
  partialRollback: vi.fn(),
}));

import type { DiffHunk } from "../lib/ipc";
import { usePartialApply, type PartialApplyState } from "./use-partial-apply";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** 같은 내용이라도 매번 새 배열·새 객체 — 5초 폴링이 IPC에서 받아오는 상황을 흉내낸다. */
function hunks(ids: string[]): DiffHunk[] {
  return ids.map((id) => ({
    id,
    path: "a.ts",
    old_range: [1, 1],
    new_range: [1, 1],
    lines: [{ kind: "add", text: "x" }],
    protected: false,
    committed: false,
    risk: "low",
  }));
}

let container: HTMLDivElement | null = null;
let root: Root | null = null;
let latest: PartialApplyState | null = null;

function Probe({ value }: { value: DiffHunk[] }) {
  latest = usePartialApply({ host: "local", id: 7 }, value, () => {});
  return null;
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  latest = null;
  vi.clearAllMocks();
});

describe("usePartialApply selection stability", () => {
  it("keeps the user's deselection when polling returns the same hunks", async () => {
    const first = hunks(["h1", "h2"]);
    await act(async () => root?.render(<Probe value={first} />));
    expect(latest?.selection).toEqual(new Set(["h1", "h2"]));

    await act(async () => latest?.toggle(first[0]));
    expect(latest?.selection).toEqual(new Set(["h2"]));

    // 5초 폴링: 내용은 같고 참조만 새로운 배열이 들어온다.
    await act(async () => root?.render(<Probe value={hunks(["h1", "h2"])} />));
    expect(latest?.selection).toEqual(new Set(["h2"]));
  });

  it("recomputes the default selection when the hunk set actually changes", async () => {
    await act(async () => root?.render(<Probe value={hunks(["h1", "h2"])} />));
    await act(async () => latest?.toggle(hunks(["h1"])[0]));
    expect(latest?.selection).toEqual(new Set(["h2"]));

    await act(async () => root?.render(<Probe value={hunks(["h1", "h2", "h3"])} />));
    expect(latest?.selection).toEqual(new Set(["h1", "h2", "h3"]));
  });
});
