/**
 * `taskListAll`이 무엇을 "실패"로 알리는지 — 사이드바 빨간 카드의 원천이다.
 *
 * 훑는 대상은 레지스트리에 올라 있는 호스트뿐이므로, 여기 실린 호스트는 모두 연결을
 * 기대한 호스트다. 응답하지 않으면 실패로 알리고 죽은 transport는 레지스트리에서 내린다.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "task_list") return [];
    throw new Error(`unexpected command: ${cmd}`);
  }),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import { taskListAll } from "./ipc";
import { hasHost, listHosts, registerTransport, unregisterTransport, type PraxisTransport } from "./transport";
import { tauriTransport } from "./transport/tauri";

const remote = (hostId: string, taskList: () => Promise<never>): PraxisTransport =>
  ({ ...tauriTransport, kind: "remote", hostId, taskList }) as PraxisTransport;

const dead = async (): Promise<never> => { throw new Error("Runner 요청 실패 (502)"); };

beforeEach(() => {
  for (const host of listHosts()) if (host !== "local") unregisterTransport(host);
});

describe("taskListAll의 실패 집계", () => {
  it("로컬만 붙어 있으면 실패가 없다", async () => {
    const merged = await taskListAll();

    expect(merged.failures).toEqual([]);
  });

  it("붙어 있다가 응답을 멈춘 호스트는 실패로 알리고 레지스트리에서 내린다", async () => {
    registerTransport(remote("mini1", dead));

    const merged = await taskListAll();

    expect(merged.failures.map((failure) => failure.host)).toEqual(["mini1"]);
    expect(hasHost("mini1")).toBe(false);

    // 내려간 뒤에는 훑을 대상이 아니다 — 같은 호스트를 영원히 다시 물어보지 않는다.
    const second = await taskListAll();
    expect(second.failures).toEqual([]);
  });

  it("응답이 돌아오는 사이 교체된 transport의 결과는 후임을 덮지 않는다", async () => {
    let release = () => {};
    const slow = {
      ...tauriTransport,
      kind: "remote",
      hostId: "mini1",
      taskList: () => new Promise<[]>((resolve) => { release = () => resolve([]); }),
    } as unknown as PraxisTransport;
    const fresh = { ...tauriTransport, kind: "remote", hostId: "mini1", taskList: async () => [] } as PraxisTransport;
    registerTransport(slow);

    const pending = taskListAll();
    // 같은 호스트에 새 transport가 올라온 뒤에야 옛 조회가 끝난다.
    registerTransport(fresh);
    release();

    const merged = await pending;
    expect(merged.failures.map((failure) => failure.error)).toEqual(["연결이 변경되었습니다"]);
    // 후임은 그대로 살아 있어야 한다 — 옛 응답이 내려버리면 방금 붙인 연결이 사라진다.
    expect(hasHost("mini1")).toBe(true);
  });
});
