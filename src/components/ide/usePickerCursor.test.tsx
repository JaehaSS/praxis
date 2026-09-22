// @vitest-environment jsdom

import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePickerCursor } from "./usePickerCursor";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

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
});

const ITEMS = ["a", "b", "c", "d"];

interface HarnessProps {
  resetOn: "open" | "open+query";
  /** 질의가 없을 때 커서가 설 자리 — 실제 피커의 "현재 선택 행"에 해당한다. */
  initialWhenEmpty?: number;
  onCommit: (item: string | undefined) => void;
  onClose?: () => void;
}

/** 훅만 두른 최소 화면 — 필터는 접두사 하나로 충분하다. */
function Harness({ resetOn, initialWhenEmpty = 0, onCommit, onClose = () => undefined }: HarnessProps) {
  const [open, setOpen] = useState(true);
  const [query, setQuery] = useState("");
  const shown = query ? ITEMS.filter((i) => i.startsWith(query)) : ITEMS;
  const { cursor, inputRef, listRef, onKeyDown } = usePickerCursor({
    open,
    count: shown.length,
    initial: query ? 0 : initialWhenEmpty,
    resetOn,
    query,
    onCommit: (i) => onCommit(shown[i]),
    onClose: () => {
      setOpen(false);
      onClose();
    },
  });
  if (!open) return <div data-testid="closed" />;
  return (
    <div>
      <input
        ref={inputRef}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={onKeyDown}
      />
      <div ref={listRef} data-cursor-index={cursor}>
        {shown.map((item, i) => (
          <div key={item} data-cursor={i === cursor ? "true" : undefined}>
            {item}
          </div>
        ))}
      </div>
    </div>
  );
}

const input = () => container?.querySelector("input");
const cursorAt = () => container?.querySelector("[data-cursor-index]")?.getAttribute("data-cursor-index");

async function render(node: React.ReactElement): Promise<void> {
  await act(async () => root?.render(node));
}

async function type(text: string): Promise<void> {
  const el = input();
  if (!el) throw new Error("검색 필드가 없다");
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  setter?.call(el, text);
  await act(async () => el.dispatchEvent(new Event("input", { bubbles: true })));
}

async function press(key: string): Promise<void> {
  await act(async () => input()?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true })));
}

describe("usePickerCursor", () => {
  it("열리면 검색 필드에 포커스가 간다", async () => {
    await render(<Harness resetOn="open" onCommit={() => undefined} />);
    expect(document.activeElement).toBe(input());
  });

  it("열 때 커서는 준 자리에 선다", async () => {
    await render(<Harness resetOn="open" initialWhenEmpty={2} onCommit={() => undefined} />);
    expect(cursorAt()).toBe("2");
  });

  it("↑↓는 경계에서 순환하지 않는다", async () => {
    await render(<Harness resetOn="open" onCommit={() => undefined} />);
    await press("ArrowUp");
    expect(cursorAt()).toBe("0");
    for (let i = 0; i < 6; i += 1) await press("ArrowDown");
    expect(cursorAt()).toBe("3");
  });

  it("Enter는 커서 행을 확정한다", async () => {
    const onCommit = vi.fn();
    await render(<Harness resetOn="open" onCommit={onCommit} />);
    await press("ArrowDown");
    await press("Enter");
    expect(onCommit).toHaveBeenCalledWith("b");
  });

  it("Esc는 한 번에 닫는다 — 질의를 먼저 비우지 않는다", async () => {
    const onClose = vi.fn();
    await render(<Harness resetOn="open" onCommit={() => undefined} onClose={onClose} />);
    await type("b");
    await press("Escape");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("resetOn open — 질의가 바뀌어도 커서를 되돌리지 않는다", async () => {
    await render(<Harness resetOn="open" initialWhenEmpty={2} onCommit={() => undefined} />);
    await type("");
    expect(cursorAt()).toBe("2");
  });

  it("resetOn open+query — 질의가 바뀌면 첫 매치로 간다", async () => {
    await render(<Harness resetOn="open+query" initialWhenEmpty={2} onCommit={() => undefined} />);
    expect(cursorAt()).toBe("2");
    await type("c");
    expect(cursorAt()).toBe("0");
  });

  it("커서를 클램프하지 않는다 — 범위 밖이면 확정할 행이 없다", async () => {
    const onCommit = vi.fn();
    await render(<Harness resetOn="open" initialWhenEmpty={3} onCommit={onCommit} />);
    await type("a");
    expect(cursorAt()).toBe("3");
    await press("Enter");
    expect(onCommit).toHaveBeenCalledWith(undefined);
  });
});
