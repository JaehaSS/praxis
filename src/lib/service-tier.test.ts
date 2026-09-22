import { expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({ invoke: vi.fn().mockResolvedValue({ id: 7, service_tier: "fast" }) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: api.invoke }));
import { taskServiceTierSet } from "./ipc";

it("saves the local task speed with the camelCase IPC contract", async () => {
  expect(await taskServiceTierSet({ host: "local", id: 7 }, "fast")).toMatchObject({ service_tier: "fast" });
  expect(api.invoke).toHaveBeenCalledWith("task_service_tier_set", { id: 7, serviceTier: "fast" });
});

it("rejects a remote task before invoking a same-numbered local task", async () => {
  api.invoke.mockClear();
  await expect(taskServiceTierSet({ host: "runner:test", id: 7 }, "fast")).rejects.toThrow("로컬");
  expect(api.invoke).not.toHaveBeenCalled();
});
