// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LspTarget } from "../../lib/ipc";
import { ReferencesPanel } from "./ReferencesPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const target = (line: number): LspTarget => ({
  path: "src/호출.ts",
  abs_path: "/work/src/호출.ts",
  line,
  column: 3,
  external: false,
});

let container: HTMLDivElement;
let root: Root;

function render(targets: LspTarget[], onOpen = vi.fn(), onClose = vi.fn()) {
  act(() => {
    root.render(<ReferencesPanel targets={targets} onOpen={onOpen} onClose={onClose} />);
  });
  return { onOpen, onClose };
}

function key(key: string) {
  act(() => {
    container.querySelector<HTMLElement>("[role=dialog]")?.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key }),
    );
  });
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("ReferencesPanel", () => {
  it("keeps an empty response visible", () => {
    render([]);

    expect(container.textContent).toContain("사용처 · 0");
    expect(container.textContent).toContain("사용처를 찾지 못했습니다");
    expect(document.activeElement).toBe(container.querySelector('[role="dialog"]'));
  });

  it("opens the keyboard-selected result and closes with Escape", () => {
    const { onOpen, onClose } = render([target(4), target(8)]);

    key("ArrowDown");
    key("Enter");
    key("Escape");

    expect(onOpen).toHaveBeenCalledWith(target(8));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("loads only the selected preview and rejects an older selection response", async () => {
    let resolveFirst!: (text: string) => void;
    const readPreview = vi.fn().mockImplementationOnce(() => new Promise<string>((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValue("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve");
    await act(async () => root.render(<ReferencesPanel targets={[target(4), target(8)]} readPreview={readPreview} onOpen={vi.fn()} onClose={vi.fn()} />));
    expect(readPreview).toHaveBeenCalledTimes(1);
    key("ArrowDown");
    await act(async () => undefined);
    expect(readPreview).toHaveBeenCalledTimes(2);
    expect(container.querySelector("pre")?.textContent).toContain("8  eight");
    expect(container.querySelector("pre")?.textContent).not.toContain("1  one");
    await act(async () => resolveFirst("old selection"));
    expect(container.querySelector("pre")?.textContent).not.toContain("old selection");
  });

  it("retains locations on preview failure and never reads an external target", async () => {
    const readPreview = vi.fn().mockRejectedValue(new Error("file missing"));
    const onOpen = vi.fn();
    const external = { ...target(8), external: true, path: null };
    await act(async () => root.render(<ReferencesPanel targets={[target(4), external]} readPreview={readPreview} onOpen={onOpen} onClose={vi.fn()} />));
    expect(container.textContent).toContain("미리보기를 읽지 못했습니다");
    expect(container.querySelectorAll('[role="option"]')).toHaveLength(2);
    key("ArrowDown");
    key("Enter");
    expect(readPreview).toHaveBeenCalledTimes(1);
    expect(onOpen).toHaveBeenCalledWith(external);
  });
});
