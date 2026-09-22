import { describe, expect, it } from "vitest";
import {
  EMPTY_CONTEXT_OBSERVATION,
  foldContextObservation,
  MODEL_CHANGE_INVALIDATION,
} from "./context-observation";

const codex = (over: Record<string, unknown> = {}) => ({
  kind: "context_usage",
  context_tokens: 147_043,
  context_window: 258_400,
  observed_at: 1_725_000_000,
  source: "codex_session",
  valid: true,
  ...over,
});

describe("foldContextObservation", () => {
  it("last call and its actual Codex window replace the legacy cumulative total", () => {
    const out = foldContextObservation(
      EMPTY_CONTEXT_OBSERVATION,
      [{ kind: "context_usage", context_tokens: 7_451_604 }, codex()],
      "codex",
    );
    expect(out.observation).toEqual({
      contextTokens: 147_043,
      contextWindow: 258_400,
      observedAt: 1_725_000_000,
      source: "codex_session",
    });
  });

  it("rejects malformed or old Codex observations", () => {
    for (const event of [
      codex({ context_window: -1 }),
      codex({ context_tokens: Infinity }),
      codex({ observed_at: Number.MAX_SAFE_INTEGER }),
      { kind: "context_usage", context_tokens: 7_451_604 },
    ])
      expect(
        foldContextObservation(EMPTY_CONTEXT_OBSERVATION, [event], "codex").observation,
      ).toBeNull();
  });

  it("clears on reset and accepts a later valid observation", () => {
    const out = foldContextObservation(
      EMPTY_CONTEXT_OBSERVATION,
      [codex(), { kind: "context_cleared" }, codex({ context_tokens: 2_000 })],
      "codex",
    );
    expect(out.observation?.contextTokens).toBe(2_000);
  });

  it("uses the same last observation for full history and remote continuation batches", () => {
    const events = [codex(), { kind: "text" }, codex({ context_tokens: 2_000 })];
    const history = foldContextObservation(EMPTY_CONTEXT_OBSERVATION, events, "codex");
    const continuation = foldContextObservation(
      foldContextObservation(EMPTY_CONTEXT_OBSERVATION, events.slice(0, 2), "codex"),
      events.slice(2),
      "codex",
    );
    expect(continuation).toEqual(history);
  });

  it("keeps a model-change invalidation until the next invocation", () => {
    const out = foldContextObservation(EMPTY_CONTEXT_OBSERVATION, [
      codex(),
      { kind: "context_usage", context_tokens: 0, source: "model_change", valid: false },
      codex({ context_tokens: 100_000 }),
      { kind: "context_usage", context_tokens: 0, source: "codex_session", valid: false },
    ], "codex");
    expect(out).toEqual(MODEL_CHANGE_INVALIDATION);
    const restored = foldContextObservation(
      out,
      [{ kind: "model_snapshot", source: "invocation" }, codex({ context_tokens: 2_000 })],
      "codex",
    );
    expect(restored.observation?.contextTokens).toBe(2_000);
  });

  it("reconstructs Claude legacy values with the model-derived window", () => {
    const out = foldContextObservation(
      EMPTY_CONTEXT_OBSERVATION,
      [{ kind: "context_usage", context_tokens: 200_001 }],
      "claude",
    );
    expect(out.observation?.source).toBe("claude_legacy");
  });
});
