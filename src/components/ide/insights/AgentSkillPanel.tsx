import { useEffect, useState, type ReactElement } from "react";
import {
  insightsAgentSkills,
  type AgentSkillStat,
  type AgentSkillUsage,
  type InsightsRange,
} from "../../../lib/ipc";
import { fmtInt, fmtPct, fmtTokens } from "./format";
import { RankBar } from "./parts";
import { StackedBar } from "./StackedBar";

/** 에이전트 카드 한 장에 펼쳐 보일 스킬 수. 넘으면 "외 N개"로 접는다. */
const TOP_SKILLS = 6;

/** 표에 세울 스킬 수 — 꼬리는 길고 대부분 한두 번 쓰인 것들이다. */
const TOP_SKILL_ROWS = 12;

/** 에이전트 키 → 표시명. 서브에이전트는 agentType 원문이 곧 이름이라 그대로 쓴다. */
function agentLabel(agent: string): string {
  if (agent === "main") return "메인 세션";
  if (agent === "unknown") return "미상";
  return agent;
}

interface PanelProps {
  range: InsightsRange;
}

interface StateProps {
  data: AgentSkillUsage | null;
  loading: boolean;
  error: string | null;
}

export function AgentSkillPanel({ range }: PanelProps): ReactElement {
  const [data, setData] = useState<AgentSkillUsage | null>(null);
  const [loading, setLoading] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    insightsAgentSkills(range)
      .then((result) => active && setData(result))
      .catch((reason: unknown) => active && setError(String(reason)))
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, [range]);

  return <AgentSkillPanelState data={data} loading={loading} error={error} />;
}

export function AgentSkillPanelState({ data, loading, error }: StateProps): ReactElement {
  if (loading) return <PanelMessage>스킬 사용 집계 중…</PanelMessage>;
  if (error || !data) {
    return (
      <PanelMessage alert>
        스킬 통계를 불러오지 못했습니다. 나머지 인사이트는 계속 사용할 수 있습니다.
      </PanelMessage>
    );
  }
  if (data.agents.length === 0) {
    return <PanelMessage>스킬 사용 기록이 없습니다.</PanelMessage>;
  }

  const maxAgentTokens = Math.max(...data.agents.map((a) => a.tokens), 1);

  return (
    <>
      <div className="bg-surface border border-border rounded-lg p-4 mb-2.5">
        <div className="text-xs text-text-secondary mb-2">스킬 점유율 (토큰)</div>
        <StackedBar
          segments={data.skills.map((s) => ({ key: s.skill, label: s.skill, value: s.tokens }))}
        />
      </div>

      <div className="bg-surface border border-border rounded-lg mb-2.5">
        <div className="text-xs text-text-secondary px-3 pt-3 pb-1">에이전트별 스킬</div>
        {data.agents.map((a) => (
          <AgentRow key={a.agent} agent={a} max={maxAgentTokens} />
        ))}
      </div>

      <div className="bg-surface border border-border rounded-lg overflow-x-auto">
        <table className="w-full text-sm min-w-[520px]">
          <thead>
            <tr className="text-xs text-text-secondary border-b border-border">
              <th className="text-left font-medium px-3 py-2">스킬</th>
              <th className="text-right font-medium px-3 py-2">토큰</th>
              <th className="text-right font-medium px-3 py-2">호출</th>
              <th className="text-right font-medium px-3 py-2">세션</th>
              <th className="text-right font-medium px-3 py-2">메시지</th>
              <th className="text-left font-medium px-3 py-2">주 에이전트</th>
            </tr>
          </thead>
          <tbody>
            {data.skills.slice(0, TOP_SKILL_ROWS).map((s) => (
              <tr key={s.skill} className="border-b border-border last:border-0">
                <td className="px-3 py-2 truncate max-w-[200px]" title={s.skill}>
                  {s.skill}
                </td>
                <td className="px-3 py-2 text-right font-code">{fmtTokens(s.tokens)}</td>
                <td className="px-3 py-2 text-right font-code text-text-secondary">
                  {s.calls === 0 ? "—" : fmtInt(s.calls)}
                </td>
                <td className="px-3 py-2 text-right font-code text-text-secondary">
                  {fmtInt(s.sessions)}
                </td>
                <td className="px-3 py-2 text-right font-code text-text-secondary">
                  {fmtInt(s.messages)}
                </td>
                <td className="px-3 py-2 text-text-secondary truncate max-w-[160px]">
                  {s.agents.length === 0 ? "—" : agentLabel(s.agents[0].agent)}
                  {s.agents.length > 1 && (
                    <span className="text-text-muted"> 외 {s.agents.length - 1}</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <p className="text-xs text-text-muted mt-2 leading-relaxed">
        토큰·메시지는 각 메시지에 붙은 스킬 귀속으로 집계한다. 호출은 <code>Skill</code> 도구를 거친
        명시적 호출만 세므로, 슬래시 입력이나 자동 매칭으로 발동한 스킬은 호출이 0이어도 작업량은 잡힌다.
      </p>
    </>
  );
}

/** 에이전트 한 종 — 합계 막대 + 그 안에서 돈 스킬 분해. */
function AgentRow({ agent, max }: { agent: AgentSkillStat; max: number }): ReactElement {
  const hidden = Math.max(0, agent.skills.length - TOP_SKILLS);
  const attributed = agent.messages - agent.unattributed_messages;
  return (
    <div className="px-3 py-2 border-t border-border">
      <div className="flex items-baseline justify-between gap-3 mb-1.5">
        <span className="truncate" title={agent.agent}>
          {agentLabel(agent.agent)}
        </span>
        <span className="text-xs font-code text-text-secondary shrink-0">
          {fmtTokens(agent.tokens)} · {fmtInt(agent.runs)}
          {agent.agent === "main" ? "세션" : "회 스폰"} · 스킬 비중{" "}
          {fmtPct(attributed, agent.messages)}
        </span>
      </div>
      <RankBar value={agent.tokens} max={max} />
      {agent.skills.length === 0 ? (
        <div className="text-xs text-text-muted mt-2">스킬 귀속 없음</div>
      ) : (
        <div className="flex flex-wrap gap-1.5 mt-2">
          {agent.skills.slice(0, TOP_SKILLS).map((s) => (
            <span
              key={s.skill}
              className="text-xs font-code text-text-secondary bg-raised rounded px-1.5 py-0.5"
              title={`${s.skill} · ${fmtInt(s.messages)}메시지 · 호출 ${fmtInt(s.calls)}회`}
            >
              {s.skill} <span className="text-text-muted">{fmtTokens(s.tokens)}</span>
            </span>
          ))}
          {hidden > 0 && <span className="text-xs text-text-muted px-1 py-0.5">외 {hidden}개</span>}
        </div>
      )}
    </div>
  );
}

function PanelMessage({
  children,
  alert,
}: {
  children: ReactElement | string;
  alert?: boolean;
}): ReactElement {
  return (
    <div
      className={`bg-surface border border-border rounded-lg p-4 text-sm ${
        alert ? "text-text-secondary" : "text-text-muted"
      }`}
    >
      {children}
    </div>
  );
}
