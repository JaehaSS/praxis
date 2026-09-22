// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

vi.mock("./knowledge-vault/VaultView", () => ({ VaultView: ({ host, agent, initialTab, repo }: { host: string; agent?: string; initialTab?: string; repo?: string }) => <p>{`창고 ${host} ${agent ?? ""} ${initialTab ?? "note"} ${repo ?? ""}`}</p> }));
import { WikiView } from "./WikiView";

let node: HTMLDivElement; let root: Root;
beforeEach(() => { node = document.createElement("div"); document.body.appendChild(node); root = createRoot(node); });
afterEach(() => { act(() => root.unmount()); node.remove(); });

it("renders the vault and offers no legacy wiki toggle", async () => {
  // 채널이 답하는 질문은 하나다 — 구 위키 색인은 설정 › 지식 그래프로 갔다(계획 2026-09-13).
  await act(async () => { root.render(<WikiView repo="/repo" host="local" agent="codex" />); });

  expect(node.textContent).toContain("창고 local codex note /repo");
  expect(node.querySelectorAll("button")).toHaveLength(0);
});

it("passes an outside tab request through to the vault", async () => {
  await act(async () => { root.render(<WikiView host="local" initialTab="memory" />); });

  expect(node.textContent).toContain("memory");
});
