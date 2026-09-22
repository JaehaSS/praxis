import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AgentSkillUsage, SkillSlice } from "../../../lib/ipc";
import { AgentSkillPanelState } from "./AgentSkillPanel";

const slice = (skill: string, tokens: number, calls = 0): SkillSlice => ({
  skill,
  calls,
  messages: Math.max(1, Math.round(tokens / 1000)),
  tokens,
});

const baseline: AgentSkillUsage = {
  agents: [
    {
      agent: "implementer",
      runs: 12,
      calls: 0,
      messages: 800,
      tokens: 9_000_000,
      unattributed_messages: 200,
      skills: [slice("verification-loop", 6_000_000), slice("feature-development", 3_000_000)],
    },
    {
      agent: "main",
      runs: 40,
      calls: 5,
      messages: 400,
      tokens: 2_000_000,
      unattributed_messages: 0,
      skills: [slice("github-operator", 2_000_000, 5)],
    },
  ],
  skills: [
    {
      skill: "verification-loop",
      calls: 0,
      messages: 600,
      tokens: 6_000_000,
      sessions: 9,
      agents: [{ agent: "implementer", calls: 0, messages: 600, tokens: 6_000_000 }],
    },
    {
      skill: "feature-development",
      calls: 0,
      messages: 200,
      tokens: 3_000_000,
      sessions: 4,
      agents: [{ agent: "implementer", calls: 0, messages: 200, tokens: 3_000_000 }],
    },
    {
      skill: "github-operator",
      calls: 5,
      messages: 400,
      tokens: 2_000_000,
      sessions: 7,
      agents: [
        { agent: "main", calls: 5, messages: 300, tokens: 1_500_000 },
        { agent: "general-purpose", calls: 0, messages: 100, tokens: 500_000 },
      ],
    },
  ],
  attributed_messages: 1000,
  unattributed_messages: 200,
  subagent_runs: 12,
};

const render = (data: AgentSkillUsage | null, loading = false, error: string | null = null) =>
  renderToStaticMarkup(<AgentSkillPanelState data={data} loading={loading} error={error} />);

describe("AgentSkillPanelState", () => {
  it("에이전트별로 어떤 스킬을 돌렸는지 펼친다", () => {
    const html = render(baseline);

    expect(html).toContain("implementer");
    expect(html).toContain("메인 세션"); // main은 표시명으로 바꾼다
    expect(html).toContain("verification-loop");
    expect(html).toContain("github-operator");
    expect(html).toContain("9.0M");
    expect(html).toContain("12회"); // 스폰 횟수
  });

  it("스킬에 귀속되지 않은 메시지를 비중으로 드러낸다", () => {
    const html = render(baseline);
    // implementer는 800건 중 600건만 스킬 귀속 → 75%
    expect(html).toContain("75%");
    // main은 전량 귀속 → 100%
    expect(html).toContain("100%");
  });

  it("명시적 호출이 없는 스킬은 호출 칸을 비운다", () => {
    const html = render(baseline);
    // github-operator만 calls=5. 나머지는 "—"로 남아 0회와 미측정을 섞지 않는다.
    expect(html).toContain("<td class=\"px-3 py-2 text-right font-code text-text-secondary\">—</td>");
    expect(html).toContain("주 에이전트");
    expect(html).toContain("외 1"); // github-operator는 에이전트 2종
  });

  it("에이전트당 스킬이 많으면 접는다", () => {
    const many = {
      ...baseline,
      agents: [
        {
          ...baseline.agents[0],
          skills: Array.from({ length: 9 }, (_, i) => slice(`skill-${i}`, 1_000_000 - i)),
        },
      ],
    };
    const html = render(many);
    expect(html).toContain("skill-5");
    expect(html).not.toContain("skill-6");
    expect(html).toContain("외 3개");
  });

  it("기록이 없으면 빈 상태를 보인다", () => {
    const html = render({ ...baseline, agents: [], skills: [] });
    expect(html).toContain("스킬 사용 기록이 없습니다");
  });

  it("실패해도 나머지 인사이트를 막지 않는다고 알린다", () => {
    const html = render(null, false, "boom");
    expect(html).toContain("불러오지 못했습니다");
    expect(html).toContain("나머지 인사이트는 계속 사용할 수 있습니다");
  });

  it("로딩 중임을 알린다", () => {
    expect(render(null, true)).toContain("집계 중");
  });
});
