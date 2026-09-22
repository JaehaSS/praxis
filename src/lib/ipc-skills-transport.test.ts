import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { skillsList } from "./ipc";
import { registerTransport, type PraxisTransport } from "./transport";
import { tauriTransport } from "./transport/tauri";

describe("skills IPC transport routing", () => {
  it("원격 세션의 `/스킬` 목록은 그 호스트에서 온다 — 로컬 스캔이 아니다", async () => {
    const skillsListRemote = vi.fn(async () => []);
    const remote: PraxisTransport = {
      ...tauriTransport,
      kind: "remote",
      hostId: "mini1",
      skillsList: skillsListRemote,
    };
    registerTransport(remote);

    await skillsList("mini1", "/runner/repo");

    expect(skillsListRemote).toHaveBeenCalledWith("/runner/repo");
  });

  it("조회가 실패해도 빈 목록으로 내려앉는다 — 자동완성이 입력을 막지 않는다", async () => {
    const remote: PraxisTransport = {
      ...tauriTransport,
      kind: "remote",
      hostId: "mini2",
      skillsList: async () => {
        throw new Error("Runner 요청 실패 (403)");
      },
    };
    registerTransport(remote);

    await expect(skillsList("mini2", "/elsewhere")).resolves.toEqual([]);
  });
});
