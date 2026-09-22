// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { HostScopeProvider, useHostScope } from "./host-scope";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Probe() {
  return <span data-testid="host">{useHostScope()}</span>;
}

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
  container = null;
  root = null;
});

const render = async (node: React.ReactNode) => {
  await act(async () => {
    root?.render(node);
    await Promise.resolve();
  });
};

describe("호스트 스코프", () => {
  it("Provider 밖에서는 로컬을 본다 — 안전한 기본값", async () => {
    await render(<Probe />);

    expect(container?.textContent).toBe("local");
  });

  it("주입한 호스트를 그대로 읽는다", async () => {
    await render(
      <HostScopeProvider value="mini1">
        <Probe />
      </HostScopeProvider>,
    );

    expect(container?.textContent).toBe("mini1");
  });

  it("서브트리마다 다른 호스트를 볼 수 있다 — 전역이 아니라 스코프다", async () => {
    await render(
      <HostScopeProvider value="mini1">
        <Probe />
        <HostScopeProvider value="box2">
          <Probe />
        </HostScopeProvider>
      </HostScopeProvider>,
    );

    expect(container?.textContent).toBe("mini1box2");
  });
});
