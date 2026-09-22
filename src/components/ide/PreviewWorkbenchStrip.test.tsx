// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PreviewWorkbenchStrip } from "./PreviewWorkbenchStrip";
import type { PreviewWorkbenchState } from "../../lib/preview-workbench/types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const state = (patch: Partial<PreviewWorkbenchState> = {}): PreviewWorkbenchState => ({
  key: "local:1", taskId: 1, appEpoch: "e", busy: "idle", url: "http://localhost:3000",
  convoActive: false, takenOver: false, supported: true, unsupportedReason: null, draft: "질문", displayUrl: "http://localhost:3000",
  pending: null, inFlight: null, error: null, lastAction: null, revision: 1, ...patch,
});

let root: Root | null = null;
let host: HTMLDivElement | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove(); root = null; host = null;
});

async function render(value = state()) {
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  await act(async () => root?.render(<PreviewWorkbenchStrip state={value} onDraftChange={vi.fn()} onSubmit={vi.fn()} onCancelPending={vi.fn()} onTakeOver={vi.fn()} onRelease={vi.fn()} onRefresh={vi.fn()} />));
}

describe("PreviewWorkbenchStrip", () => {
  it("does not submit Enter while composing and preserves Shift+Enter", async () => {
    const submit = vi.fn();
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    await act(async () => root?.render(<PreviewWorkbenchStrip state={state()} onDraftChange={vi.fn()} onSubmit={submit} onCancelPending={vi.fn()} onTakeOver={vi.fn()} onRelease={vi.fn()} onRefresh={vi.fn()} />));
    const input = host.querySelector("textarea")!;
    await act(async () => input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true })));
    await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    await act(async () => input.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true })));
    await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true })));
    expect(submit).not.toHaveBeenCalled();
  });

  it("submits an unmodified Enter after composition ends", async () => {
    const submit = vi.fn();
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    await act(async () => root?.render(<PreviewWorkbenchStrip state={state()} onDraftChange={vi.fn()} onSubmit={submit} onCancelPending={vi.fn()} onTakeOver={vi.fn()} onRelease={vi.fn()} onRefresh={vi.fn()} />));
    const input = host.querySelector("textarea")!;
    await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })));
    expect(submit).toHaveBeenCalledWith("질문");
  });

  it("allows a manual question while takeover is active", async () => {
    await render(state({ takenOver: true }));
    expect(host?.querySelector<HTMLButtonElement>("button")?.disabled).toBe(false);
    expect(host?.textContent).toContain("돌려주기");
  });

  it("explains unsupported work without enabling send", async () => {
    await render(state({ supported: false, unsupportedReason: "원격 작업은 지원하지 않습니다." }));
    expect(host?.textContent).toContain("원격 작업은 지원하지 않습니다.");
    expect([...host!.querySelectorAll("button")].find((button) => button.textContent === "보내기")?.disabled).toBe(true);
  });

  it("allows a supported local request while busy state is unknown", async () => {
    await render(state({ busy: "unknown", error: "다시 확인하세요." }));
    expect([...host!.querySelectorAll("button")].find((button) => button.textContent === "대기 등록")?.disabled).toBe(false);
    expect(host?.textContent).toContain("다시 확인하세요.");
  });

  it("labels an unknown request as queued while state is checked", async () => {
    await render(state({ busy: "unknown" }));
    expect(host?.textContent).toContain("실행 상태 확인 중");
    expect(host?.textContent).toContain("대기 등록");
  });

  it("keeps compact toolbar controls in a single bounded row", async () => {
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    await act(async () => root?.render(<PreviewWorkbenchStrip compact state={state()} onDraftChange={vi.fn()} onSubmit={vi.fn()} onCancelPending={vi.fn()} onTakeOver={vi.fn()} onRelease={vi.fn()} onRefresh={vi.fn()} />));
    expect(host.querySelector("textarea")?.className).toContain("resize-none");
    expect(host.querySelector("textarea")?.className).toContain("min-w-0");
    expect(host.querySelector("textarea")?.parentElement?.className).toContain("flex-row");
  });
});
