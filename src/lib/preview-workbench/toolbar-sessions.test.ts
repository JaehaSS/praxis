import { describe, expect, it } from "vitest";
import { ToolbarSessions } from "./toolbar-sessions";
import type { PreviewWorkbenchState } from "./types";

const state = { revision: 1 } as PreviewWorkbenchState;
const message = (label = "bar-1", generation = 1) => ({ kind: "ready" as const, appEpoch: "epoch", taskId: 1, toolbarLabel: label, windowGeneration: generation, correlationId: "ready" });
const theme = { id: "praxis-dark" } as never;

describe("ToolbarSessions", () => {
  it("replaces a closed task session and rejects old identities", () => {
    const sessions = new ToolbarSessions();
    const old = sessions.register(message());
    sessions.register(message("bar-2", 2));
    expect(sessions.all()).toHaveLength(1);
    expect(sessions.get({ ...old, kind: "intent" })).toBeUndefined();
  });

  it("publishes a replacement generation even when its state revision restarts", () => {
    const sessions = new ToolbarSessions();
    const first = sessions.register(message());
    expect(sessions.publication(first, state, theme)).toBe(1);
    sessions.commitPublication(first, state, theme, 1);
    const replacement = sessions.register(message("bar-1", 2));
    expect(sessions.publication(replacement, state, theme)).toBeGreaterThan(1);
  });

  it("publishes only changed snapshots or themes and clears closed sessions", () => {
    const sessions = new ToolbarSessions();
    const session = sessions.register(message());
    expect(sessions.publication(session, state, theme)).toBe(1);
    sessions.commitPublication(session, state, theme, 1);
    expect(sessions.publication(session, state, theme)).toBeNull();
    expect(sessions.publication(session, state, { id: "praxis-light" } as never)).toBe(2);
    sessions.commitPublication(session, state, { id: "praxis-light" } as never, 2);
    expect(sessions.publication(session, state, { id: "praxis-light", tokens: { bg: "#000" } } as never)).toBe(3);
    sessions.commitPublication(session, state, { id: "praxis-light", tokens: { bg: "#000" } } as never, 3);
    expect(sessions.publication(session, state, { id: "praxis-light", tokens: { bg: "#000" } } as never)).toBeNull();
    sessions.close(session.toolbarLabel);
    expect(sessions.all()).toEqual([]);
    expect(sessions.publication(session, state, theme)).toBeNull();
  });
});
