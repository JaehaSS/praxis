import { useCallback, useEffect, useState } from "react";
import { mcpList, mcpAdd, mcpRemove, mcpSetEnabled, type McpServer } from "../lib/ipc";
import { inputCls } from "./ide/formStyles";
import { ToggleBadge, ResourceRow, DeleteButton } from "./ide/ResourceRow";

/** 흔한 MCP 서버 프리셋 (@modelcontextprotocol/server-*). 모두 npx 실행.
 *  `note`는 추가 직후 안내로 띄운다 — 별도 준비가 필요한 서버만 채운다. */
const PRESETS: { name: string; args: string[]; note?: string }[] = [
  { name: "github", args: ["-y", "@modelcontextprotocol/server-github"] },
  { name: "filesystem", args: ["-y", "@modelcontextprotocol/server-filesystem", "."] },
  { name: "slack", args: ["-y", "@modelcontextprotocol/server-slack"] },
  { name: "brave-search", args: ["-y", "@modelcontextprotocol/server-brave-search"] },
  { name: "memory", args: ["-y", "@modelcontextprotocol/server-memory"] },
  { name: "sequential-thinking", args: ["-y", "@modelcontextprotocol/server-sequential-thinking"] },
  // 에이전트가 브라우저를 직접 조작한다. `--headless`를 붙이지 않는 것이 의도다 — 브라우저 창이
  // 실제로 떠야 사용자가 에이전트의 조작을 곁에서 지켜볼 수 있다.
  { name: "playwright", args: ["-y", "@playwright/mcp@latest"] },
  // 같은 자리의 대안. 스냅샷의 접근성 트리에 `@e1` 같은 고정 ref를 붙여 셀렉터 없이 조작하고,
  // 출력 상한과 페이지 내용 경계 표시를 기본으로 낀다. playwright를 대체하지 않고 나란히 둔다 —
  // .mcp.json은 작업 생성 시 1회만 기재되므로, 지우면 이미 쓰던 작업만 깨진다.
  {
    name: "agent-browser",
    args: ["-y", "agent-browser", "mcp"],
    note: "첫 사용 전 `npx agent-browser install`로 Chrome을 받아야 합니다.",
  },
  {
    name: "lsp-rust",
    args: [
      "-y",
      "--silent",
      "git+https://github.com/jonrad/lsp-mcp#b48c04c52731e3e499352fc644992dcce6202db2",
      "--lsp",
      "rust-analyzer",
    ],
  },
  {
    name: "lsp-typescript",
    args: [
      "-y",
      "--silent",
      "git+https://github.com/jonrad/lsp-mcp#b48c04c52731e3e499352fc644992dcce6202db2",
      "--lsp",
      "npx -y --silent typescript-language-server --stdio",
    ],
  },
  {
    name: "lsp-python",
    args: [
      "-y",
      "--silent",
      "git+https://github.com/jonrad/lsp-mcp#b48c04c52731e3e499352fc644992dcce6202db2",
      "--lsp",
      "pyright-langserver --stdio",
    ],
  },
];

/** Phase 7 MCP 레지스트리 — 등록 서버는 작업 worktree의 .mcp.json으로 저장됨. */
export function McpServersView() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [err, setErr] = useState<string | null>(null);
  // 프리셋이 별도 준비를 요구할 때만 찬다. 추가 직후가 사용자가 그것을 알아야 할 시점이다.
  const [notice, setNotice] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("npx");
  const [args, setArgs] = useState('["-y","@modelcontextprotocol/server-github"]');

  const refresh = useCallback(async () => {
    try {
      setServers(await mcpList());
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const add = async () => {
    setErr(null);
    try {
      await mcpAdd(name.trim(), command.trim(), args.trim());
      setName("");
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  return (
    <div className="flex-1 overflow-auto p-4">
      {err && <div className="text-status-failed text-sm font-code mb-2">{err}</div>}
      <div className="max-w-3xl mx-auto">
        <div className="text-text-muted text-xs mb-3">
          MCP 서버를 등록하면 새 작업의 worktree에 <span className="font-code">.mcp.json</span>으로 저장되어
          에이전트가 그대로 사용합니다. (시크릿은 저장하지 않음 — 환경변수 상속)
        </div>

        {notice && (
          <div className="bg-surface border border-border rounded-lg p-3 mb-3 flex items-start gap-2">
            <div className="text-text-secondary text-xs flex-1">{notice}</div>
            <button
              className="text-text-muted text-xs hover:text-text-secondary"
              onClick={() => setNotice(null)}
            >
              닫기
            </button>
          </div>
        )}

        {/* 등록 폼 */}
        <div className="bg-surface border border-border rounded-lg p-3 mb-4 flex flex-wrap gap-2 items-center">
          <input className={`${inputCls} w-32`} placeholder="name" value={name} onChange={(e) => setName(e.target.value)} />
          <input className={`${inputCls} w-28`} placeholder="command" value={command} onChange={(e) => setCommand(e.target.value)} />
          <input className={`${inputCls} flex-1 min-w-48`} placeholder='args (JSON 배열)' value={args} onChange={(e) => setArgs(e.target.value)} />
          <button
            className="h-9 px-3 rounded-md bg-primary text-bg text-sm font-medium disabled:bg-border disabled:text-text-muted"
            disabled={!name.trim() || !command.trim()}
            onClick={add}
          >
            추가
          </button>
        </div>

        {/* 프리셋 */}
        <div className="flex flex-wrap items-center gap-2 mb-4">
          <span className="text-text-muted text-xs">프리셋:</span>
          {PRESETS.map((p) => (
            <button
              key={p.name}
              className="h-7 px-2 rounded-md bg-surface border border-border text-text-secondary text-xs hover:border-border-strong"
              onClick={() =>
                mcpAdd(p.name, "npx", JSON.stringify(p.args))
                  .then(async () => {
                    await refresh();
                    setNotice(p.note ?? null);
                  })
                  .catch((e) => setErr(String(e)))
              }
            >
              + {p.name}
            </button>
          ))}
        </div>

        {servers.length === 0 ? (
          <div className="text-text-muted text-center py-8">등록된 MCP 서버가 없습니다.</div>
        ) : (
          <div className="flex flex-col gap-2">
            {servers.map((s) => (
              <ResourceRow key={s.id}>
                <ToggleBadge enabled={!!s.enabled} onToggle={() => mcpSetEnabled(s.id, s.enabled === 0).then(refresh)} />
                <span className="font-medium text-sm w-28 truncate">{s.name}</span>
                <span className="font-code text-text-secondary text-xs flex-1 truncate">
                  {s.command} {s.args}
                </span>
                <DeleteButton onRemove={() => mcpRemove(s.id).then(refresh)} />
              </ResourceRow>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
