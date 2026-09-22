// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  annotationSave: vi.fn(async () => undefined),
}));

vi.mock("../lib/ipc", () => ({
  annotationSave: mocks.annotationSave,
  annotationsResend: vi.fn(),
}));

import { useDiffReviewActions } from "./use-diff-review-actions";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const INPUT = { hunk_id: "h1", line: 3, side: "new", body_md: "여기" };

let actions: ReturnType<typeof useDiffReviewActions> | null = null;
let container: HTMLDivElement;
let root: Root;

function Probe({ path }: { path: string | null }) {
  actions = useDiffReviewActions({ host: "local", id: 2 }, path, [], () => {});
  return null;
}

const render = async (path: string | null) => {
  await act(async () => {
    root.render(<Probe path={path} />);
    await Promise.resolve();
  });
};

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  actions = null;
  vi.clearAllMocks();
});

/** 빈 경로의 주석은 어느 파일에도 다시 붙지 않는다 — 저장하는 순간 되돌릴 수 없다. */
describe("useDiffReviewActions 주석 저장", () => {
  it("보고 있는 파일이 없으면 저장하지 않는다", async () => {
    await render(null);
    await act(async () => actions?.create(INPUT));

    expect(mocks.annotationSave).not.toHaveBeenCalled();
  });

  it("보고 있는 파일이 있으면 그 경로로 저장한다", async () => {
    await render("src/a.ts");
    await act(async () => actions?.create(INPUT));

    expect(mocks.annotationSave).toHaveBeenCalledWith(
      { host: "local", id: 2 },
      { ...INPUT, path: "src/a.ts" },
    );
  });
});
