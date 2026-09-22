// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { GithubIssuesSection, type GithubIssuesApi } from "./GithubIssuesSection";
import { ALL_REPOS } from "./github-repos";
import type { GhIssue, GhRepo, GithubIssuesResult, Task } from "../../lib/ipc";

// 실제 opener는 Tauri 런타임을 요구한다 — 모듈 로드 자체를 막는다(호출은 api.open으로 주입).
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const repo = (path: string, ownerRepo: string): GhRepo => ({ path, owner_repo: ownerRepo });

const issue = (over: Partial<GhIssue>): GhIssue => ({
  number: 7,
  title: "이슈 제목",
  labels: [],
  updated_at: "2026-08-03T00:00:00Z",
  ...over,
});

const REPOS = [repo("/work/app", "acme/app"), repo("/work/site", "acme/site")];

const ready = (ownerRepo: string, issues: GhIssue[]): GithubIssuesResult => ({
  status: "ready",
  owner_repo: ownerRepo,
  issues,
});

/** 경로마다 다른 응답을 주는 `list` — 레포별 조회를 검증하려면 응답도 레포별이어야 한다. */
const listing = (byPath: Record<string, GithubIssuesResult>) =>
  vi.fn((path: string) => Promise.resolve(byPath[path] ?? ready("acme/none", [])));

let container: HTMLDivElement | null = null;
let root: Root | null = null;
let api: GithubIssuesApi;
let onSelectRepo: ReturnType<typeof vi.fn>;

const mocked = (fn: unknown): ReturnType<typeof vi.fn> => fn as ReturnType<typeof vi.fn>;

async function render(
  props: Partial<Parameters<typeof GithubIssuesSection>[0]> = {},
): Promise<void> {
  await act(async () => {
    root?.render(
      <GithubIssuesSection
        repos={REPOS}
        repo="/work/app"
        onSelectRepo={onSelectRepo}
        onOpenTask={vi.fn()}
        onRefresh={vi.fn()}
        api={api}
        {...props}
      />,
    );
    // 후보 전부를 병렬 조회하므로 마이크로태스크 한 틱으로는 모자란다.
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function buttonByLabel(label: string): HTMLButtonElement | null {
  return container?.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`) ?? null;
}

function buttonByText(text: string): HTMLButtonElement | null {
  const buttons = [...(container?.querySelectorAll("button") ?? [])];
  return (buttons.find((b) => b.textContent?.trim() === text) as HTMLButtonElement) ?? null;
}

async function click(element: Element | null): Promise<void> {
  await act(async () => {
    element?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await Promise.resolve();
  });
}

const text = (): string => container?.textContent ?? "";

beforeEach(() => {
  onSelectRepo = vi.fn();
  api = {
    list: listing({
      "/work/app": ready("acme/app", [issue({ number: 7 }), issue({ number: 9, title: "다른 이슈" })]),
      "/work/site": ready("acme/site", [issue({ number: 3, title: "사이트 이슈" })]),
    }),
    create: vi.fn().mockResolvedValue({ id: 1 } as Task),
    remove: vi.fn().mockResolvedValue(undefined),
    open: vi.fn(),
  };
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

describe("GithubIssuesSection", () => {
  it("어느 레포의 목록인지와 무엇을 보여주는지를 헤더에 밝힌다", async () => {
    await render();
    expect(text()).toContain("acme/app");
    expect(text()).toContain("열린 이슈 2건");
  });

  it("후보 레포마다 버튼을 두고, 누르면 선택을 위로 올린다", async () => {
    await render();
    expect(buttonByText("app")).not.toBeNull();
    expect(buttonByText("site")).not.toBeNull();

    await click(buttonByText("site"));
    expect(onSelectRepo).toHaveBeenCalledWith("/work/site");
  });

  it("선택된 레포 버튼만 눌린 상태로 표시된다", async () => {
    await render();
    expect(buttonByText("app")?.getAttribute("aria-pressed")).toBe("true");
    expect(buttonByText("site")?.getAttribute("aria-pressed")).toBe("false");
  });

  it("후보가 하나뿐이면 전환 버튼을 만들지 않는다 — 고를 것이 없다", async () => {
    await render({ repos: [REPOS[0]] });
    expect(container?.querySelector('[aria-label="레포 선택"]')).toBeNull();
    expect(text()).toContain("acme/app");
  });

  it("후보 전부를 한 번에 조회하고, 레포를 바꿔도 다시 부르지 않는다", async () => {
    await render();
    expect(mocked(api.list).mock.calls).toEqual([["/work/app"], ["/work/site"]]);

    await render({ repo: "/work/site" });
    expect(mocked(api.list).mock.calls).toHaveLength(2);
    expect(text()).toContain("사이트 이슈");
  });

  it("열린 이슈가 없는 레포는 버튼에서 뺀다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", [issue({ number: 7 })]),
      "/work/site": ready("acme/site", []),
      "/work/docs": ready("acme/docs", [issue({ number: 5 })]),
    });
    await render({ repos: [...REPOS, repo("/work/docs", "acme/docs")] });
    expect(buttonByText("app")).not.toBeNull();
    expect(buttonByText("docs")).not.toBeNull();
    expect(buttonByText("site")).toBeNull();
  });

  it("판정 전에는 레포 버튼을 하나도 세우지 않는다 — 있던 것이 사라지는 장면을 보이지 않는다", async () => {
    api.list = vi.fn(() => new Promise<GithubIssuesResult>(() => {}));
    await render();
    expect(container?.querySelector('[aria-label="레포 선택"]')).toBeNull();
    expect(buttonByText("app")).toBeNull();
    expect(text()).toContain("이슈를 불러오는 중");
  });

  it("후보가 바뀌면 낡은 결과로 버튼을 그리지 않는다", async () => {
    await render();
    expect(buttonByText("app")).not.toBeNull();

    // 새 후보의 응답이 오기 전까지는 판정 전 — 이전 후보의 결과를 재활용하지 않는다.
    api.list = vi.fn(() => new Promise<GithubIssuesResult>(() => {}));
    await render({ repos: [...REPOS, repo("/work/docs", "acme/docs")] });
    expect(container?.querySelector('[aria-label="레포 선택"]')).toBeNull();
  });

  it("새로고침 중에도 목록을 비우지 않는다 — 버튼만 진행 중임을 알린다", async () => {
    await render();
    // 새로고침은 후보 전부를 다시 부른다 — 하나만 풀면 Promise.all이 끝나지 않는다.
    const pending: Array<(result: GithubIssuesResult) => void> = [];
    api.list = vi.fn(() => new Promise<GithubIssuesResult>((resolve) => pending.push(resolve)));

    const refresh = container?.querySelector('[aria-label="새로고침"]') ?? null;
    await click(refresh);
    expect(text()).toContain("이슈 제목");
    expect(buttonByText("app")).not.toBeNull();
    expect(refresh?.getAttribute("aria-busy")).toBe("true");

    await act(async () => {
      pending.forEach((resolve) => resolve(ready("acme/app", [issue({ number: 7 })])));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(refresh?.getAttribute("aria-busy")).toBe("false");
  });

  it("걸러내고 하나만 남으면 전환 버튼을 접는다 — 고를 것이 없다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", [issue({ number: 7 })]),
      "/work/site": ready("acme/site", []),
    });
    await render();
    expect(container?.querySelector('[aria-label="레포 선택"]')).toBeNull();
    expect(text()).toContain("acme/app");
  });

  it("보고 있던 레포가 걸러지면 이슈가 있는 레포로 옮긴다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", []),
      "/work/site": ready("acme/site", [issue({ number: 3 })]),
    });
    await render();
    expect(onSelectRepo).toHaveBeenCalledWith("/work/site");
  });

  it("조회에 실패한 레포도 버튼에서 뺀다 — 눌러도 이슈가 나오지 않는다", async () => {
    api.list = vi.fn((path: string) =>
      path === "/work/site"
        ? Promise.reject(new Error("gh 실패"))
        : Promise.resolve(ready(path === "/work/app" ? "acme/app" : "acme/docs", [issue({})])),
    );
    await render({ repos: [...REPOS, repo("/work/docs", "acme/docs")] });
    expect(buttonByText("app")).not.toBeNull();
    expect(buttonByText("docs")).not.toBeNull();
    expect(buttonByText("site")).toBeNull();
  });

  it("어느 레포에도 열린 이슈가 없으면 섹션을 통째로 접는다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", []),
      "/work/site": ready("acme/site", []),
    });
    await render();
    expect(text()).toBe("");
  });

  it("조회에 실패했으면 접지 않는다 — 고칠 수 있는 사정을 말할 자리를 남긴다", async () => {
    api.list = vi.fn((path: string) =>
      path === "/work/app"
        ? Promise.resolve(ready("acme/app", []))
        : Promise.reject(new Error("gh 실패")),
    );
    await render();
    expect(text()).toContain("GitHub 이슈");
    expect(text()).toContain("열린 이슈가 있는 레포가 없습니다");
  });

  it("이슈 번호를 누르면 GitHub 이슈 페이지를 연다", async () => {
    await render();
    await click(buttonByText("#7"));
    expect(api.open).toHaveBeenCalledWith("https://github.com/acme/app/issues/7");
  });

  it("보고 있는 레포만 비었으면 그 이름을 대고 말한다 — 다른 레포에는 이슈가 있다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", []),
      "/work/site": ready("acme/site", [issue({ number: 3 })]),
    });
    await render();
    expect(text()).toContain("acme/app에 열린 이슈가 없습니다");
  });

  it("gh 미인증은 에러가 아니라 안내로 처리한다 — 후보를 지우지도 않는다", async () => {
    api.list = vi.fn(() => Promise.resolve({ status: "unavailable" }) as Promise<GithubIssuesResult>);
    await render();
    expect(text()).toContain("gh auth login");
    expect(buttonByText("site")).not.toBeNull();
  });

  it("GitHub 레포 후보가 없으면 섹션을 그리지 않는다", async () => {
    await render({ repos: [], repo: undefined });
    expect(text()).toBe("");
  });
});

describe("GithubIssuesSection — 전체 보기", () => {
  it("레포 버튼 줄 맨 앞에 '전체'를 두고, 누르면 전체 선택을 위로 올린다", async () => {
    await render();
    const group = container?.querySelector('[aria-label="레포 선택"]');
    expect(group?.firstElementChild?.textContent?.trim()).toBe("전체");

    await click(buttonByText("전체"));
    expect(onSelectRepo).toHaveBeenCalledWith(ALL_REPOS);
  });

  it("후보가 하나뿐이면 '전체'도 만들지 않는다 — 합칠 것이 없다", async () => {
    await render({ repos: [REPOS[0]] });
    expect(buttonByText("전체")).toBeNull();
  });

  it("전체는 후보의 이슈를 한 목록으로 합치되 다시 부르지 않는다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", [issue({ number: 7, updated_at: "2026-08-01T00:00:00Z" })]),
      "/work/site": ready("acme/site", [
        issue({ number: 3, title: "사이트 이슈", updated_at: "2026-08-10T00:00:00Z" }),
      ]),
    });
    await render({ repo: ALL_REPOS });
    expect(mocked(api.list).mock.calls).toHaveLength(2);
    expect(text()).toContain("이슈 제목");
    expect(text()).toContain("사이트 이슈");
    expect(text()).toContain("열린 이슈 2건");
    // 최근 갱신이 위 — 레포별로 이어 붙이면 오늘 것이 반년 전 것 아래로 간다.
    expect(text().indexOf("사이트 이슈")).toBeLessThan(text().indexOf("이슈 제목"));
  });

  it("전체 보기에서는 줄마다 어느 레포인지 밝힌다", async () => {
    await render({ repo: ALL_REPOS });
    expect(buttonByText("전체")?.getAttribute("aria-pressed")).toBe("true");
    expect(text()).toContain("app");
    expect(text()).toContain("site");
  });

  it("전체 보기는 레포를 옮기라고 요구하지 않는다 — 어느 레포도 가리키지 않는다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", []),
      "/work/site": ready("acme/site", [issue({ number: 3 })]),
    });
    await render({ repo: ALL_REPOS });
    expect(onSelectRepo).not.toHaveBeenCalled();
  });

  it("전체 보기의 태스크 생성은 그 줄의 레포로 간다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", [issue({ number: 7, updated_at: "2026-08-01T00:00:00Z" })]),
      "/work/site": ready("acme/site", [issue({ number: 3, updated_at: "2026-08-10T00:00:00Z" })]),
    });
    await render({ repo: ALL_REPOS });
    // 맨 위는 최근 갱신된 site의 이슈다.
    await click([...(container?.querySelectorAll("button") ?? [])].find(
      (button) => button.textContent?.trim() === "태스크 생성",
    ) ?? null);
    expect(api.create).toHaveBeenCalledWith("/work/site", 3, "claude");
  });
});

describe("GithubIssuesSection — 이슈 삭제", () => {
  it("한 번 더 확인받기 전에는 지우지 않는다 — 되돌릴 수 없는 동작이다", async () => {
    await render();
    await click(buttonByLabel("#7 삭제"));
    expect(api.remove).not.toHaveBeenCalled();
    expect(text()).toContain("되돌릴 수 없습니다");

    await click(buttonByText("취소"));
    expect(api.remove).not.toHaveBeenCalled();
    expect(text()).toContain("이슈 제목");
  });

  it("확인하면 지우고, 그 줄만 목록에서 뺀다 — 재조회하지 않는다", async () => {
    await render();
    await click(buttonByLabel("#7 삭제"));
    await click(buttonByText("삭제"));

    expect(api.remove).toHaveBeenCalledWith("/work/app", 7);
    expect(mocked(api.list).mock.calls).toHaveLength(2);
    expect(text()).not.toContain("#7");
    expect(text()).toContain("#9");
    expect(text()).toContain("열린 이슈 1건");
  });

  it("실패하면 사유를 그대로 남기고 이슈도 남긴다 — 권한 문제를 뭉개지 않는다", async () => {
    api.remove = vi.fn().mockRejectedValue(new Error("must have admin rights to Repository"));
    await render();
    await click(buttonByLabel("#7 삭제"));
    await click(buttonByText("삭제"));

    expect(text()).toContain("must have admin rights");
    expect(text()).toContain("#7");
  });

  it("전체 보기의 삭제는 그 줄의 레포로 간다", async () => {
    api.list = listing({
      "/work/app": ready("acme/app", [issue({ number: 7, updated_at: "2026-08-01T00:00:00Z" })]),
      "/work/site": ready("acme/site", [issue({ number: 3, updated_at: "2026-08-10T00:00:00Z" })]),
    });
    await render({ repo: ALL_REPOS });
    await click(buttonByLabel("#3 삭제"));
    await click(buttonByText("삭제"));

    expect(api.remove).toHaveBeenCalledWith("/work/site", 3);
    expect(text()).not.toContain("#3");
    expect(text()).toContain("#7");
  });
});
