// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  remove: vi.fn(),
}));

vi.mock("../../lib/ipc", () => ({
  githubIssuesList: mocks.list,
  githubCreateTaskFromIssue: mocks.create,
  githubIssueDelete: mocks.remove,
}));
// 실제 opener는 Tauri 런타임을 요구한다 — 모듈 로드 자체를 막는다.
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import { GithubIssuesSection } from "./GithubIssuesSection";
import type { GhRepo, GithubIssuesResult } from "../../lib/ipc";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const repo = (path: string, ownerRepo: string): GhRepo => ({ path, owner_repo: ownerRepo });

const REPOS = [repo("/work/app", "acme/app"), repo("/work/site", "acme/site")];

const ready = (ownerRepo: string): GithubIssuesResult => ({
  status: "ready",
  owner_repo: ownerRepo,
  issues: [
    { number: 7, title: "이슈 제목", labels: [], updated_at: "2026-08-03T00:00:00Z" },
  ],
});

let container: HTMLDivElement | null = null;
let root: Root | null = null;

/** 조회는 비동기 두 겹(Promise.all → setSnapshot)이라 마이크로태스크 한 틱으로는 모자라다. */
async function settle(rounds = 6): Promise<void> {
  for (let i = 0; i < rounds; i += 1) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
}

/** 실사용 경로 — 홈은 `api`를 주입하지 않는다. 주입하면 이 회귀가 재현되지 않는다. */
async function render(): Promise<void> {
  await act(async () => {
    root?.render(
      <GithubIssuesSection
        repos={REPOS}
        repo="/work/app"
        onSelectRepo={vi.fn()}
        onOpenTask={vi.fn()}
        onRefresh={vi.fn()}
      />,
    );
  });
}

describe("GithubIssuesSection 재조회", () => {
  beforeEach(() => {
    mocks.list.mockReset();
    mocks.list.mockImplementation((_host: unknown, path: string) =>
      Promise.resolve(ready(path === "/work/site" ? "acme/site" : "acme/app")),
    );
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    container = null;
    root = null;
  });

  // 기본 api가 매 렌더 새 객체이면 load → effect → 렌더가 서로를 깨워 조회가 멈추지 않는다.
  // 홈에서 목록이 계속 다시 그려지며 깜빡이던 것이 이 루프였다.
  it("api를 주입하지 않아도 후보당 한 번만 조회한다", async () => {
    await render();
    await settle();

    expect(mocks.list).toHaveBeenCalledTimes(REPOS.length);
  });

  // 부모(홈)는 작업 목록 갱신마다 다시 그린다 — 그때마다 gh를 다시 부르면 안 된다.
  it("같은 후보로 다시 그려도 재조회하지 않는다", async () => {
    await render();
    await settle();
    mocks.list.mockClear();

    await render();
    await settle();

    expect(mocks.list).not.toHaveBeenCalled();
  });
});
