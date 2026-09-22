import { RunnerTransport } from "../lib/transport/runner";
import { classifyStatus, type ConnectionState } from "./status";

// 모바일은 셸을 서빙한 그 오리진에 그대로 붙는다. pairingToken 없이 HttpOnly 세션 쿠키로
// 인증하므로 RunnerTransport의 `/v1` 계약을 전부 그대로 재사용할 수 있다. (설계 0013 §6.1)
export const api = new RunnerTransport({ endpoint: window.location.origin });

/** 지금 붙을 수 있는지, 못 붙는다면 왜인지 판정한다. (설계 0013 §7.2) */
export async function probeConnection(): Promise<ConnectionState> {
  try {
    const response = await fetch("/v1/health", { credentials: "same-origin" });
    if (response.ok) return { kind: "ok", health: await response.json() };
    const kind = classifyStatus(response.status);
    if (kind === "unauthorized") return { kind: "unauthorized" };
    if (kind === "runner-down") return { kind: "runner-down", status: response.status };
    return { kind: "error", status: response.status };
  } catch {
    // fetch가 던지는 건 DNS·TLS·네트워크 단계 실패다 — 폰이 tailnet 밖에 있는 경우.
    return { kind: "offline" };
  }
}
