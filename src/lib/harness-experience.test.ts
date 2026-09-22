import { describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { loadExperiences, readExperience } from "./harness-experience";

describe("harness experience service", () => {
  it("blocks remote list and read before invoking Tauri", async () => {
    await expect(
      loadExperiences("remote", "/repo", "workflow-harness"),
    ).resolves.toEqual({ state: "unsupported", reason: "remote-host" });
    await expect(
      readExperience(
        "remote",
        "/repo",
        { kind: "project", project: { host: "local", projectKey: "/repo" } },
        "project-lessons",
      ),
    ).resolves.toEqual({ state: "unsupported", reason: "remote-host" });
    expect(invoke).not.toHaveBeenCalled();
  });

  it("uses the command DTO argument names for local lists", async () => {
    invoke.mockResolvedValueOnce({ state: "ready" });
    await loadExperiences("local", "/repo", "loop-engineering");
    expect(invoke).toHaveBeenCalledWith("harness_experience_list", {
      repo: "/repo",
      harness: "loop-engineering",
    });
  });

  it("uses the exact command DTO for local reads", async () => {
    const owner = {
      kind: "project" as const,
      project: { host: "local" as const, projectKey: "/repo" },
    };
    invoke.mockResolvedValueOnce({ state: "ready" });

    await readExperience("local", "/repo", owner, "project-lessons");

    expect(invoke).toHaveBeenCalledWith("harness_experience_read", {
      repo: "/repo",
      owner,
      documentKey: "project-lessons",
    });
  });
});
