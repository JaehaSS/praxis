import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Task } from "../../lib/ipc";
import { ActivityColumnTab, ActivityRail } from "./ActivityRail";

const task: Task = {
  id: 7,
  host: "local",
  repo: "/workspace/praxis",
  branch: "feature/floating-work-context",
  base: "main",
  worktree_path: "/workspace/praxis/.praxis/worktrees/task-7",
  instruction: "작업 정보를 세션 위 플로팅 채널로 표시한다",
  state: "Running",
  created_at: 1,
  updated_at: 2,
  mode: "conversation",
};

const props = {
  task,
  runtime: "local" as const,
  diff: { state: "ready", value: "1 file changed, 4 insertions(+), 1 deletion(-)" } as const,
  items: [],
  busy: true,
  activity: null,
  onRefreshDiff: () => {},
  onOpenSubagent: () => {},
  onOpenRecentActivity: () => {},
};

const render = (): string =>
  renderToStaticMarkup(
    <ActivityRail
      task={task}
      runtime="local"
      diff={{ state: "ready", value: "1 file changed, 4 insertions(+), 1 deletion(-)" }}
      items={[]}
      busy
      activity={null}
      onRefreshDiff={() => {}}
      onOpenSubagent={() => {}}
      onOpenRecentActivity={() => {}}
    />,
  );

describe("ActivityRail", () => {
  it("세션 폭을 차지하지 않는 우상단 채널 스택으로 렌더한다", () => {
    const stack = render().match(/<div[^>]+>/)?.[0] ?? "";

    expect(stack).toContain("absolute");
    expect(stack).toContain("top-4");
    expect(stack).toContain("right-4");
    expect(stack).toContain("z-30");
    // 컨테이너는 아래까지 내려오되 클릭도 배경도 받지 않는다 — 카드 사이 공백은 세션 그대로다.
    expect(stack).toContain("bottom-4");
    expect(stack).toContain("pointer-events-none");
    expect(stack).toContain("flex-col");
    expect(stack).not.toContain("bg-raised");
  });

  it("작업정보 카드는 콘텐츠 높이로 접히고 공간이 모자라면 먼저 줄어든다", () => {
    const card = render().match(/<aside[^>]+>/)?.[0] ?? "";

    expect(card).toContain('aria-label="플로팅 작업 정보"');
    expect(card).toContain("rounded-xl");
    expect(card).toContain("bg-raised");
    expect(card).toContain("shadow-");
    expect(card).toContain("pointer-events-auto");
    // 줄어드는 쪽은 작업정보다(min-h-0 + 내부 스크롤). 카드가 세로를 다 채우면 안 된다.
    expect(card).toContain("min-h-0");
    expect(card).not.toContain("flex-1");
    expect(card).not.toContain("border-l");
  });
});

describe("ActivityColumnTab", () => {
  const markup = () => renderToStaticMarkup(<ActivityColumnTab {...props} />);

  it("코드 열 안에서는 떠 있지 않고 자리를 받아 흐른다", () => {
    const host = markup().match(/<div[^>]+>/)?.[0] ?? "";

    // absolute·pointer-events-none은 세션 위에 겹칠 때의 장치다 — 열 안에서는 쓰지 않는다.
    expect(host).not.toContain("absolute");
    expect(host).not.toContain("pointer-events-none");
    expect(host).toContain("flex-1");
    expect(host).toContain("overflow-y-auto");
  });

  it("플로팅과 같은 카드를 쓴다 — 옮겨 온 것이지 다른 화면이 아니다", () => {
    expect(markup()).toContain('aria-label="작업 정보"');
  });

  it("열 폭을 온전히 쓰므로 최근 활동을 목록째 펼친다(panel 밀도)", () => {
    // rail 밀도는 최근 활동을 진입점 한 줄로 접는다. 여기서는 그 접기가 없어야 한다.
    expect(markup()).not.toContain("최근 활동 0건");
  });
});

describe("작업정보 접기", () => {
  it("떠 있는 채널은 접기를 환경 헤더까지 내려보낸다 — 소환면은 자기 위에 닫기를 갖는다", () => {
    const html = renderToStaticMarkup(<ActivityRail {...props} onCollapse={() => {}} />);

    expect(html).toContain('aria-label="작업정보 접기"');
  });

  it("접기를 받지 않은 채널에는 닫기가 없다", () => {
    expect(renderToStaticMarkup(<ActivityRail {...props} />)).not.toContain("작업정보 접기");
  });

  it("코드 열 탭은 접기를 받아도 내지 않는다 — 고정 탭은 닫을 수 없다", () => {
    const html = renderToStaticMarkup(<ActivityColumnTab {...props} onCollapse={() => {}} />);

    expect(html).not.toContain("작업정보 접기");
    // 카드 자체는 그대로 있다 — 닫기만 없는 것이지 패널이 비는 것이 아니다.
    expect(html).toContain('aria-label="작업 정보"');
  });
});
