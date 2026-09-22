import { useEffect, useState } from "react";
import { AGENT_PRESETS } from "../lib/agents";
import { api } from "./api";
import { navigate } from "./router";
import { taskHref } from "./routes";
import { Button, Empty, Spinner } from "./primitives";

// 새 작업 생성 — 레포·에이전트·지시문. (설계 0013 §5.3)
// 데스크톱 컴포저의 고급 옵션(앙상블·모델 오버라이드·Goal Contract)은 폰에서 다루기
// 어렵고 오조작 비용이 크다. 여기서는 세 가지만 받고 나머지는 서버 기본값에 맡긴다.

/** 좁은 화면에서 전체 경로는 읽히지 않는다 — 마지막 세그먼트만 크게 보여준다. */
function repoName(repo: string): string {
  const parts = repo.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? repo;
}

export function NewTaskScreen() {
  const [repos, setRepos] = useState<string[] | null>(null);
  const [repo, setRepo] = useState("");
  const [agent, setAgent] = useState(AGENT_PRESETS[0]?.key ?? "claude");
  const [instruction, setInstruction] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .repositoryList()
      .then((list) => {
        if (cancelled) return;
        setRepos(list);
        // 레포가 하나뿐이면 고를 이유가 없다 — 바로 채운다.
        if (list.length === 1) setRepo(list[0]);
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setRepos([]);
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const canCreate = !busy && repo.trim().length > 0 && instruction.trim().length > 0;

  const create = async () => {
    if (!canCreate) return;
    setBusy(true);
    setError(null);
    try {
      const task = await api.taskCreate({
        // 모바일은 Runner 하나에 직결이라 호스트가 곧 그 transport다.
        host: api.hostId,
        repo,
        instruction: instruction.trim(),
        agent,
        model: "",
        // 폰에는 역할 선택 UI가 없다 — 데스크톱 기본값과 같은 implementer로 보낸다.
        role: "implementer",
        // 아래 넷은 Runner의 `/v1/tasks`가 쓰지 않지만 공통 계약이 요구한다.
        headless: false,
        ensemble: "",
        mode: "conversation",
        cmd: "",
        args: [],
        cols: 80,
        rows: 24,
      });
      // 생성 직후 상세로 보낸다 — 폰에서는 "만들고 끝"이 아니라 바로 지켜보게 된다.
      navigate(taskHref(task.id));
    } catch (cause: unknown) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setBusy(false);
    }
  };

  if (!repos) return <Spinner label="레포를 불러오는 중" />;

  return (
    <div className="space-y-5 px-4 py-4">
      <label className="block space-y-1.5">
        <span className="text-xs text-text-muted">레포</span>
        {repos.length === 0 ? (
          <Empty>Runner가 레포를 찾지 못했습니다.</Empty>
        ) : (
          <select
            value={repo}
            onChange={(event) => setRepo(event.target.value)}
            aria-label="레포"
            className="min-h-[44px] w-full rounded-lg border border-border bg-surface px-3 text-sm text-text"
          >
            <option value="">선택하세요</option>
            {repos.map((path) => (
              <option key={path} value={path}>
                {repoName(path)}
              </option>
            ))}
          </select>
        )}
        {repo ? (
          <span className="block break-all text-[11px] text-text-muted">{repo}</span>
        ) : null}
      </label>

      <div className="space-y-1.5">
        <span className="text-xs text-text-muted">에이전트</span>
        {/* 드롭다운보다 칩이 낫다 — 선택지가 적고 한 번에 다 보인다. */}
        <div className="flex flex-wrap gap-2">
          {AGENT_PRESETS.map((preset) => (
            <button
              key={preset.key}
              type="button"
              onClick={() => setAgent(preset.key)}
              aria-pressed={agent === preset.key}
              className={`min-h-[44px] rounded-lg border px-3 text-sm ${
                agent === preset.key
                  ? "border-primary text-primary-bright"
                  : "border-border text-text-muted"
              }`}
            >
              {preset.badge}
            </button>
          ))}
        </div>
      </div>

      <label className="block space-y-1.5">
        <span className="text-xs text-text-muted">지시문</span>
        <textarea
          value={instruction}
          onChange={(event) => setInstruction(event.target.value)}
          rows={6}
          aria-label="지시문"
          placeholder="무엇을 시킬지 적으세요"
          className="w-full rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text placeholder:text-text-muted"
        />
      </label>

      {error ? <div className="text-sm text-status-failed">{error}</div> : null}

      <Button variant="primary" disabled={!canCreate} onClick={() => void create()}>
        {busy ? "만드는 중…" : "작업 시작"}
      </Button>
    </div>
  );
}
