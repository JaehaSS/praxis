import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import {
  contextFileRead,
  contextReport,
  memoryAdd,
  memoryList,
  memoryRestoreVersion,
  memoryVersions,
} from "./ipc";
import { registerTransport, type PraxisTransport } from "./transport";
import { tauriTransport } from "./transport/tauri";

describe("memory IPC transport routing", () => {
  it("uses the selected remote transport instead of local Tauri", async () => {
    const memoryListRemote = vi.fn(async () => []);
    const memoryAddRemote = vi.fn(async () => 9);
    const contextReportRemote = vi.fn(async () => ({
      vendors: [],
      injected: [],
      capture_enabled: false,
      memory_count: 0,
    }));
    const contextFileReadRemote = vi.fn(async () => "remote context");
    const memoryVersionsRemote = vi.fn(async () => []);
    const memoryRestoreVersionRemote = vi.fn(async () => 3);
    const remote: PraxisTransport = {
      ...tauriTransport,
      kind: "remote",
      hostId: "mini1",
      memoryList: memoryListRemote,
      memoryAdd: memoryAddRemote,
      memoryVersions: memoryVersionsRemote,
      memoryRestoreVersion: memoryRestoreVersionRemote,
      contextReport: contextReportRemote,
      contextFileRead: contextFileReadRemote,
    };
    registerTransport(remote);

    await memoryList("mini1");
    await memoryAdd("mini1", "/runner/repo", "decision", "remember");
    await contextReport({ host: "mini1", id: 7 });
    await contextFileRead({ host: "mini1", id: 7 }, "/runner/repo/CLAUDE.md");
    await memoryVersions("mini1", 9);
    await memoryRestoreVersion("mini1", 9, 1, 2, "candidate");

    expect(memoryListRemote).toHaveBeenCalledOnce();
    expect(memoryAddRemote).toHaveBeenCalledWith("/runner/repo", "decision", "remember");
    expect(contextReportRemote).toHaveBeenCalledWith(7);
    expect(contextFileReadRemote).toHaveBeenCalledWith(7, "/runner/repo/CLAUDE.md");
    expect(memoryVersionsRemote).toHaveBeenCalledWith(9);
    expect(memoryRestoreVersionRemote).toHaveBeenCalledWith(9, 1, 2, "candidate");
  });
});
