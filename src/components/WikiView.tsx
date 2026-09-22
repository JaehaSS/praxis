import { LOCAL_HOST, type HostId } from "../lib/transport";
import { VaultView } from "./knowledge-vault/VaultView";

/**
 * Wiki 채널 — 답하는 질문은 하나다: "내 창고 폴더에 무엇이 있고, 어떻게 손보나."
 * 구 위키(Obsidian 색인) 토글은 여기서 내려가고 설정 › 지식 그래프로 옮겼다(계획 2026-09-13).
 */
export function WikiView({ repo, host = LOCAL_HOST, agent = "claude", initialTab, taskId = null }: { repo?: string; host?: HostId; agent?: string; initialTab?: "memory"; taskId?: number | null }) {
  return <div className="min-h-0 min-w-0 flex-1 overflow-auto p-4"><VaultView repo={repo} host={host} agent={agent} initialTab={initialTab} taskId={taskId} /></div>;
}
