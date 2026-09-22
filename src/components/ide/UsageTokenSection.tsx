import { useCallback, useEffect, useState, type ReactElement } from "react";
import {
  usageClaudeTokenClear,
  usageClaudeTokenSet,
  usageClaudeTokenStatus,
  usageSnapshot,
} from "../../lib/ipc";

const input =
  "flex-1 bg-bg border border-border rounded px-2 py-1.5 text-sm text-text outline-none focus:border-primary";
const button =
  "text-[11px] px-1.5 py-0.5 rounded border border-border text-text-secondary " +
  "hover:bg-raised hover:text-text disabled:opacity-40 disabled:hover:bg-transparent";

/**
 * Claude 사용량 조회용 장기 토큰(`claude setup-token`) 등록.
 *
 * 저장이 곧 검증이다 — 이 엔드포인트가 장기 토큰을 받는지 확인되지 않았으므로, 실제로
 * 조회해 본 토큰만 키체인에 들어간다. 저장된 값은 어떤 경로로도 화면에 돌아오지 않는다.
 */
export function UsageTokenSection(): ReactElement {
  const [stored, setStored] = useState<boolean | null>(null);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; failed: boolean } | null>(null);

  useEffect(() => {
    void usageClaudeTokenStatus()
      .then(setStored)
      .catch(() => setStored(null));
  }, []);

  const run = useCallback(async (action: () => Promise<unknown>, done: string, next: boolean) => {
    setBusy(true);
    setMessage(null);
    try {
      await action();
      setStored(next);
      setToken("");
      setMessage({ text: done, failed: false });
      await usageSnapshot(true);
    } catch (error) {
      setMessage({ text: String(error), failed: true });
    }
    setBusy(false);
  }, []);

  return (
    <div className="space-y-2">
      <div className="text-text text-sm">Claude 사용량 조회 토큰</div>
      <div className="text-text-muted text-xs">
        <code>claude setup-token</code>으로 받은 장기 토큰을 넣으면 CLI 토큰이 만료돼도 잔량을
        계속 조회합니다. 저장 전에 실제로 조회해 보고, 되지 않는 토큰은 저장하지 않습니다.
      </div>
      <div className="flex items-center gap-2">
        <input
          type="password"
          aria-label="Claude 사용량 조회 토큰"
          autoComplete="off"
          className={input}
          placeholder={stored ? "저장됨 — 새 토큰으로 교체하려면 입력" : "sk-ant-oat…"}
          value={token}
          onChange={(event) => setToken(event.target.value)}
        />
        <button
          className={button}
          disabled={busy || token.trim() === ""}
          onClick={() =>
            void run(() => usageClaudeTokenSet(token), "토큰으로 사용량을 조회했습니다", true)
          }
        >
          저장하고 확인
        </button>
        <button
          className={button}
          disabled={busy || stored !== true}
          onClick={() => void run(usageClaudeTokenClear, "토큰을 지웠습니다", false)}
        >
          지우기
        </button>
      </div>
      <div className="text-xs text-text-muted">
        {stored === null ? "저장 여부 확인 실패" : stored ? "저장됨" : "저장된 토큰 없음"}
      </div>
      {message && (
        <div className={`text-xs ${message.failed ? "text-status-failed" : "text-text-secondary"}`}>
          {message.text}
        </div>
      )}
    </div>
  );
}
