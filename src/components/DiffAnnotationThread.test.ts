// @vitest-environment jsdom

import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAnnotationComposer, type AnnotationComposerState } from "./DiffAnnotationThread";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;
let state: AnnotationComposerState | null = null;

async function mount(
  onCreate: (input: { hunk_id: string; line: number; side: string; body_md: string }) => Promise<void>,
  onUpdate: (id: string, body: string) => Promise<void> = vi.fn(),
) {
  const Probe = () => {
    state = useAnnotationComposer(onCreate, onUpdate);
    return null;
  };
  await act(async () => root?.render(createElement(Probe)));
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
  state = null;
});

describe("useAnnotationComposer", () => {
  it("closes the composer and sends the note on a successful save", async () => {
    const onCreate = vi.fn(async () => {});
    await mount(onCreate);

    await act(async () => state?.openNew("h1", 12, "new"));
    await act(async () => state?.setText("looks wrong"));
    await act(async () => await state?.save());

    expect(onCreate).toHaveBeenCalledWith({
      hunk_id: "h1",
      line: 12,
      side: "new",
      body_md: "looks wrong",
    });
    expect(state?.activeKey).toBeNull();
    expect(state?.error).toBeNull();
  });

  it("reopens the composer with the text intact when saving fails", async () => {
    const onCreate = vi.fn(async () => {
      throw new Error("IPC down");
    });
    await mount(onCreate);

    await act(async () => state?.openNew("h1", 12, "new"));
    await act(async () => state?.setText("do not lose me"));
    await act(async () => await state?.save());

    expect(state?.activeKey).toBe("h1:12:new");
    expect(state?.text).toBe("do not lose me");
    expect(state?.error).toContain("IPC down");
  });

  it("clears a previous error when a new composer is opened", async () => {
    const onCreate = vi.fn(async () => {
      throw new Error("IPC down");
    });
    await mount(onCreate);

    await act(async () => state?.openNew("h1", 12, "new"));
    await act(async () => state?.setText("x"));
    await act(async () => await state?.save());
    expect(state?.error).not.toBeNull();

    await act(async () => state?.openNew("h1", 13, "new"));
    expect(state?.error).toBeNull();
    expect(state?.text).toBe("");
  });

  it("discards an empty note without calling the backend", async () => {
    const onCreate = vi.fn(async () => {});
    await mount(onCreate);

    await act(async () => state?.openNew("h1", 12, "new"));
    await act(async () => state?.setText("   "));
    await act(async () => await state?.save());

    expect(onCreate).not.toHaveBeenCalled();
    expect(state?.activeKey).toBeNull();
  });

  it("routes an edit to onUpdateBody instead of onCreate", async () => {
    const onCreate = vi.fn(async () => {});
    const onUpdate = vi.fn(async () => {});
    await mount(onCreate, onUpdate);

    await act(async () =>
      state?.openEdit("h1", 12, "new", {
        id: "a1",
        body_md: "old body",
      } as Parameters<AnnotationComposerState["openEdit"]>[3]),
    );
    expect(state?.text).toBe("old body");
    await act(async () => state?.setText("new body"));
    await act(async () => await state?.save());

    expect(onUpdate).toHaveBeenCalledWith("a1", "new body");
    expect(onCreate).not.toHaveBeenCalled();
  });
});
