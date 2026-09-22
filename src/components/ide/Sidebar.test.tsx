import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Task } from "../../lib/ipc";
import { Sidebar, WIKI_LOCAL_ONLY_REASON } from "./Sidebar";

const task = (overrides: Partial<Task> = {}): Task => ({
  id: 7,
  host: "local",
  repo: "/workspace/praxis",
  branch: "feature/left-task-navigation",
  base: "main",
  worktree_path: "/workspace/praxis/.praxis/worktrees/task-7",
  instruction: "세션 작업을 왼쪽으로 이동",
  state: "Running",
  created_at: 2,
  updated_at: 3,
  mode: "conversation",
  ...overrides,
});

const renderSidebar = (
  collapsed = false,
  activeState: Task["state"] = "Running",
  awaitingKind: Task["awaiting_kind"] = null,
): string =>
  renderToStaticMarkup(
    <Sidebar
      browsingHost="local"
      onPickBrowsingHost={() => {}}
      view="home"
      onNewTask={() => {}}
      onQuickLink={() => {}}
      collapsed={collapsed}
      onToggleCollapse={() => {}}
      tasks={[
        task({ state: activeState, awaiting_kind: awaitingKind }),
        task({
          id: 8,
          repo: "/workspace/archive",
          instruction: "완료된 작업",
          state: "Done",
          created_at: 1,
        }),
      ]}
      selectedKey="local:7"
      projects={["/workspace/archive", "/workspace/praxis"]}
      onOpenTask={() => {}}
      onNewInRepo={() => {}}
      onDeleteTask={() => {}}
      onRemoveProject={() => {}}
      onDiscardOrphans={() => {}}
    />,
  );

/** 그룹은 선택 prop이다 — 넘기지 않는 위 렌더가 지금까지와 같은 화면이어야 한다(A-3.1). */
const renderGrouped = (): string =>
  renderToStaticMarkup(
    <Sidebar
      browsingHost="local"
      onPickBrowsingHost={() => {}}
      view="home"
      onNewTask={() => {}}
      onQuickLink={() => {}}
      collapsed={false}
      onToggleCollapse={() => {}}
      tasks={[task()]}
      selectedKey="local:7"
      projects={["/workspace/archive", "/workspace/praxis"]}
      onOpenTask={() => {}}
      onNewInRepo={() => {}}
      onDeleteTask={() => {}}
      onRemoveProject={() => {}}
      onDiscardOrphans={() => {}}
      groups={{
        version: 1,
        groups: [{ id: "g1", name: "사내용", collapsed: false }],
        assignment: { "/workspace/praxis": "g1" },
      }}
      onProjectGroupsChange={() => {}}
    />,
  );

describe("Sidebar session task navigation", () => {
  it("Orchestrator Views 아래에 프로젝트별 세션 작업 카드를 상시 표시한다", () => {
    const html = renderSidebar();

    expect(html.indexOf("Orchestrator Views")).toBeLessThan(html.indexOf("세션 작업 (1)"));
    expect(html.indexOf("인사이트")).toBeLessThan(html.indexOf("세션 작업 (1)"));
    expect(html).toContain("rounded-lg border border-border/80");
    expect(html).toContain("bg-raised border-primary");
    expect(html).toContain("praxis");
    expect(html).toContain("세션 작업을 왼쪽으로 이동");
    expect(html).not.toContain("완료된 작업");
  });

  it("Orchestrator Views는 일상 사용 뷰 2개만 노출한다", () => {
    const html = renderSidebar();

    for (const label of ["인사이트", "Wiki"]) {
      expect(html).toContain(label);
    }
    // 설계 0016 — 자기개선은 메모리 탭으로, 관리 뷰 3종은 설정 탭으로 이동.
    // ADR 0191 — 파일 채널은 내려가고, 스킬은 설정 탭이 된다.
    // 설계 2026-09-13 — 메모리 채널도 내려가 Wiki 공간의 필터가 된다.
    // 재탐색 2026-09-13 — 리뷰 채널도 내려가 멀티벤더 리뷰는 세션 안 작업 액션이 된다.
    for (const label of ["메모리", "자기개선", "MCP 서버", "채널", "스케줄", "파일", "스킬", "리뷰"]) {
      expect(html).not.toContain(label);
    }
  });

  /** 항목을 지우면 사라진 이유가 어디에도 남지 않는다 — 자리는 지키고 잠근다. */
  const renderRemoteBrowsing = (): string =>
    renderToStaticMarkup(
      <Sidebar
        browsingHost="remote"
        onPickBrowsingHost={() => {}}
        view="home"
        onNewTask={() => {}}
        onQuickLink={() => {}}
        collapsed={false}
        onToggleCollapse={() => {}}
        tasks={[]}
        selectedKey={null}
        projects={[]}
        onOpenTask={() => {}}
        onNewInRepo={() => {}}
        onDeleteTask={() => {}}
        onRemoveProject={() => {}}
        onDiscardOrphans={() => {}}
      />,
    );

  it("원격 탐색 호스트에서는 Wiki 진입점을 지우지 않고 잠근다", () => {
    const html = renderRemoteBrowsing();

    expect(html).toContain("Wiki");
    expect(html).toContain("disabled");
    expect(html).toContain("로컬 전용");
  });

  it("잠긴 Wiki 항목은 왜 잠겼는지를 그 자리에서 밝힌다", () => {
    expect(renderRemoteBrowsing()).toContain(WIKI_LOCAL_ONLY_REASON);
  });

  it("로컬 탐색 호스트에서는 Wiki 항목이 잠기지 않는다", () => {
    const html = renderSidebar();

    expect(html).toContain("Wiki");
    expect(html).not.toContain("로컬 전용");
    expect(html).not.toContain(WIKI_LOCAL_ONLY_REASON);
  });

  it("왼쪽 사이드바를 접으면 세션 작업 목록도 함께 숨긴다", () => {
    const html = renderSidebar(true);

    expect(html).not.toContain("Orchestrator Views");
    expect(html).not.toContain("세션 작업 (1)");
    expect(html).toContain("사이드바 펼치기");
  });

  // 이전에는 점 색 + 호버 툴팁만 두고 본문 문구를 "중복"으로 보아 지웠다. 그 전제가 틀렸다 —
  // 앰버 점(검토 대기)을 완료로 오해하는 일이 실제로 발생했고, 답변 대기가 생기면서 색만으로
  // 갈라야 할 상태가 하나 더 늘었다. 색 단독 인코딩 금지(DESIGN.md Do #2)에도 어긋난다.
  // 좁은 목록을 위해 본문 문구를 축약하는 것은 무방하다 — 점에는 완전한 문구가 그대로 남는다.
  it.each([
    ["Running", null, "실행 중…", "실행 중…"],
    ["AwaitingReview", null, "검토 대기", "검토"],
    ["AwaitingReview", "question", "답변 대기", "답변"],
  ] as const)(
    "%s(%s) 상태를 점(완전 문구)과 카드 본문(축약)에 모두 적는다",
    (state, kind, full, short) => {
      const html = renderSidebar(false, state, kind);

      expect(html).toContain(`aria-label="작업 상태: ${full}"`);
      expect(html).toContain(`>${short}</span>`);
    },
  );

  // 색은 점이 진다 — 본문 문구는 브랜치명과 같은 무게로 가라앉혀 목록의 소음을 줄였다.
  it("답변 대기와 검토 대기를 점 색으로 구분한다", () => {
    expect(renderSidebar(false, "AwaitingReview", "question")).toContain("var(--c-question)");
    expect(renderSidebar(false, "AwaitingReview")).toContain("var(--c-awaiting)");
  });

  // 목록은 ACTIVE_STATES만 그리므로 종료 상태(실패·완료)는 여기 나타나지 않는다 —
  // 카드 본문에서 상태색을 쓸 이유가 없고, 쓰면 검토 대기가 줄줄이 앰버로 물든다.
  it("본문 상태 문구에는 상태색을 쓰지 않는다", () => {
    const html = renderSidebar(false, "AwaitingReview");

    expect(html).toContain(">검토</span>");
    expect(html).not.toContain("text-status-awaiting");
  });

  // 미소속에는 헤더를 두지 않는다 — 그룹을 쓰지 않는 사람의 화면까지 바뀐다.
  it("그룹 순서로 그리고 미소속 프로젝트를 그 아래 헤더 없이 둔다", () => {
    const html = renderGrouped();

    expect(html.indexOf("사내용")).toBeLessThan(html.indexOf("praxis"));
    expect(html.indexOf("praxis")).toBeLessThan(html.indexOf("archive"));
    expect(html).not.toContain("미소속");
  });

  // 카운터는 행동으로 이어지지 않는 정보였다 — 상태는 카드마다 점과 문구로 이미 적혀 있다.
  it("헤더에 실행·검토·답변 카운터를 그리지 않는다", () => {
    const html = renderSidebar(false, "AwaitingReview", "question");

    expect(html).not.toContain("text-status-question");
    expect(html).not.toContain("· 검토");
  });
});

describe("Sidebar 최근 세션", () => {
  // 최근 목록은 트리 위에 온다 — 트리를 펼쳐 찾지 않게 하는 것이 존재 이유다.
  it("1시간 안에 갱신된 작업이 있으면 세션 작업 위에 최근 세션을 그린다", () => {
    const html = renderToStaticMarkup(
      <Sidebar
        browsingHost="local"
        onPickBrowsingHost={() => {}}
        view="home"
        onNewTask={() => {}}
        onQuickLink={() => {}}
        collapsed={false}
        onToggleCollapse={() => {}}
        tasks={[task({ updated_at: Math.floor(Date.now() / 1000) })]}
        selectedKey="local:7"
        projects={["/workspace/praxis"]}
        onOpenTask={() => {}}
        onNewInRepo={() => {}}
        onDeleteTask={() => {}}
        onRemoveProject={() => {}}
        onDiscardOrphans={() => {}}
      />,
    );

    expect(html).toContain("최근 세션 (1)");
    expect(html.indexOf("최근 세션 (1)")).toBeLessThan(html.indexOf("세션 작업 ("));
  });

  // fixture의 updated_at은 epoch 3초 — 창 밖이므로 섹션 자체가 없다(결정 2).
  it("1시간 안에 갱신된 작업이 없으면 섹션을 그리지 않는다", () => {
    expect(renderSidebar()).not.toContain("최근 세션");
  });
});
