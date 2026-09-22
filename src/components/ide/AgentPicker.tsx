import { useEffect, useRef, useState } from "react";
import { Icon } from "./icons";
import { AGENT_PRESETS, labelFor } from "../../lib/agents";

interface Props {
  /** 선택된 에이전트 집합. 1개=단일 인터랙티브, 2개+=헤드리스 앙상블. */
  agents: string[];
  onChange: (agents: string[]) => void;
}

/** 리드 에이전트 선택 (멀티) — 1개면 인터랙티브 단일, 2개+면 **헤드리스 앙상블**(각자 자율수행 → 비교).
 *  프리셋 토글 + 임의 CLI용 커스텀 명령. Praxis는 에이전트 비종속. */
export function AgentPicker({ agents, onChange }: Props) {
  const [openMenu, setOpenMenu] = useState(false);
  const [custom, setCustom] = useState("");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!openMenu) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpenMenu(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [openMenu]);

  const toggle = (key: string) => {
    if (agents.includes(key)) {
      const next = agents.filter((a) => a !== key);
      onChange(next.length ? next : [key]); // 최소 1개 유지
    } else {
      onChange([...agents, key]);
    }
  };

  const addCustom = () => {
    const c = custom.trim();
    if (!c) return;
    if (!agents.includes(c)) onChange([...agents, c]);
    setCustom("");
  };

  const ensemble = agents.length > 1;
  const chipLabel = ensemble ? `${agents.length} 에이전트 · 비교 실행` : labelFor(agents[0] ?? "claude");

  return (
    <div className="relative" ref={ref}>
      <button
        className={`flex items-center gap-1 text-xs border rounded-md px-2 py-1 hover:border-border-strong ${
          ensemble ? "text-primary-bright border-primary/50" : "text-text-secondary border-border"
        }`}
        onClick={() => setOpenMenu((v) => !v)}
        title={`리드 에이전트: ${agents.join(", ")}`}
      >
        <Icon name={ensemble ? "scale" : "sparkle"} size={13} />
        <span className="max-w-[170px] truncate">{chipLabel}</span>
        <Icon name="chevronDown" size={12} />
      </button>

      {openMenu && (
        <div className="absolute bottom-full left-0 mb-1 z-20 w-72 rounded-lg border border-border-strong bg-raised py-1 shadow-xl">
          <div className="px-3 py-1 text-xs text-text-muted">
            에이전트 <span className="text-text-secondary">— 이름=단독 실행 · +비교=여러 개 비교</span>
          </div>
          {AGENT_PRESETS.map((p) => {
            const on = agents.includes(p.key);
            const solo = on && agents.length === 1;
            return (
              <div
                key={p.key}
                className={`w-full flex items-center px-2 py-1.5 text-sm hover:bg-surface ${
                  on ? "text-text" : "text-text-secondary"
                }`}
              >
                {/* 이름 클릭 = 이 벤더로 단독 선택(교체) */}
                <button
                  className="flex items-center gap-2 flex-1 min-w-0 text-left"
                  onClick={() => onChange([p.key])}
                  title="이 벤더로 단독 실행"
                >
                  <span
                    className={`shrink-0 w-4 ${solo ? "text-primary-bright" : on ? "text-primary/60" : "text-text-muted"}`}
                  >
                    <Icon name={on ? "check" : "sparkle"} size={13} />
                  </span>
                  <span className="truncate flex-1">{p.label}</span>
                  <span className="text-text-muted text-xs font-code shrink-0">{p.key}</span>
                </button>
                {/* 앙상블 토글 = 선택 유지한 채 추가/제거(2개+면 비교 실행) */}
                <button
                  className={`shrink-0 ml-1 text-xs px-1.5 py-0.5 rounded border ${
                    on && !solo
                      ? "text-primary-bright border-primary/50"
                      : "text-text-muted border-border hover:text-text"
                  }`}
                  onClick={() => toggle(p.key)}
                  title={on && !solo ? "비교에서 제거" : "비교에 추가(여러 벤더 비교)"}
                >
                  {on && !solo ? "비교 ✓" : "+비교"}
                </button>
              </div>
            );
          })}
          {/* 커스텀 명령 외 선택된 항목(있으면) */}
          {agents.filter((a) => !AGENT_PRESETS.some((p) => p.key === a)).map((c) => (
            <button
              key={c}
              className="w-full text-left flex items-center gap-2 px-3 py-1.5 text-sm text-text hover:bg-surface"
              onClick={() => toggle(c)}
              title="제거"
            >
              <span className="shrink-0 w-4 text-primary-bright">
                <Icon name="check" size={13} />
              </span>
              <span className="truncate flex-1 font-code text-xs">{c}</span>
              <span className="text-text-muted text-xs">커스텀</span>
            </button>
          ))}
          <div className="my-1 border-t border-border" />
          <div className="px-3 py-1.5">
            <div className="text-xs text-text-muted mb-1">커스텀 명령 추가</div>
            <input
              className="w-full bg-bg border border-border rounded px-2 py-1 text-sm text-text outline-none focus:border-primary placeholder:text-text-muted"
              placeholder="예: crush   ·   mybin -p {prompt}"
              value={custom}
              onChange={(e) => setCustom(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") addCustom();
              }}
            />
            <button
              className="mt-1 text-xs text-primary-bright disabled:text-text-muted"
              disabled={!custom.trim()}
              onClick={addCustom}
            >
              + 추가
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
