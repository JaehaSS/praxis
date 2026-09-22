import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { tauriTransport } from "./tauri";

describe("local review process repair transport", () => {
  beforeEach(() => invoke.mockReset());

  it("does not invent local IPC commands for the Runner-only workflow", async () => {
    await expect(tauriTransport.reviewProcessQuarantines()).resolves.toEqual([]);
    await expect(tauriTransport.reviewProcessReconcile(17)).rejects.toThrow(
      "원격 Runner 전용",
    );
    expect(invoke).not.toHaveBeenCalled();
  });
});
