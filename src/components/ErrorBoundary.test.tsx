// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ERROR_BOUNDARY_MARKER, ErrorBoundary } from "./ErrorBoundary";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  // 경계가 잡은 예외는 React가 한 번 더 콘솔에 뱉는다 — 기대된 노이즈라 여기서 삼킨다.
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.restoreAllMocks();
});

function Boom(): never {
  throw new Error("원격 세션이 터졌다");
}

const button = (text: string): HTMLButtonElement | undefined =>
  [...(container?.querySelectorAll<HTMLButtonElement>("button") ?? [])].find(
    (b) => b.textContent === text,
  );

async function render(child: React.ReactNode): Promise<void> {
  await act(async () => {
    root?.render(<ErrorBoundary>{child}</ErrorBoundary>);
  });
}

describe("ErrorBoundary", () => {
  it("자식이 던지면 빈 화면 대신 오류 메시지를 띄운다", async () => {
    await render(<Boom />);
    expect(container?.textContent).toContain("화면을 그리지 못했습니다");
    expect(container?.textContent).toContain("원격 세션이 터졌다");
  });

  it("스택을 pre로 내보내 선택·스크롤할 수 있게 한다", async () => {
    await render(<Boom />);
    const pre = container?.querySelector("pre");
    expect(pre?.textContent).toContain("원격 세션이 터졌다");
    expect(pre?.className).toContain("overflow-auto");
  });

  it("componentStack과 함께 표식을 남겨 콘솔에서 찾을 수 있게 한다", async () => {
    await render(<Boom />);
    const call = vi
      .mocked(console.error)
      .mock.calls.find((args) => args[0] === ERROR_BOUNDARY_MARKER);
    expect(call?.[1]).toBeInstanceOf(Error);
    expect(String(call?.[2])).toContain("Boom");
  });

  it("'다시 시도'는 상태를 리셋해 정상 자식을 다시 그린다", async () => {
    await render(<Boom />);
    await act(async () => root?.render(<ErrorBoundary><div>복구됨</div></ErrorBoundary>));
    await act(async () => button("다시 시도")?.click());
    expect(container?.textContent).toBe("복구됨");
  });

  it("자식이 멀쩡하면 그대로 통과시킨다", async () => {
    await render(<div>정상</div>);
    expect(container?.textContent).toBe("정상");
  });
});
