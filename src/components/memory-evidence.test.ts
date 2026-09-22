import { describe, expect, it } from "vitest";
import type { MemoryEvidence } from "../lib/ipc";
import {
  evidenceActionErrorForMemory,
  evidenceCountLabel,
  evidenceRequestBelongsToMemory,
  evidenceStateForMemory,
  evidenceLocatorLabel,
  evidenceStatusLabel,
  evidenceStatusTone,
  evidenceTimestampLabel,
  type EvidenceLoadState,
} from "./memory-evidence";

const evidence = (locator_json: string, status: MemoryEvidence["status"] = "valid") => ({
  id: 1,
  memory_id: 2,
  version: 3,
  kind: "code_location",
  locator_json,
  snapshot_hash: null,
  status,
  observed_at: 100,
  checked_at: 101,
  expires_at: null,
});

describe("memory evidence presentation", () => {
  it("shows backend locator identity without exposing raw JSON", () => {
    expect(evidenceLocatorLabel(evidence('{"relative_path":"src/lib.rs"}'))).toBe("src/lib.rs");
    expect(evidenceLocatorLabel(evidence('{"url":"https://example.test/guide"}'))).toBe(
      "https://example.test/guide",
    );
    expect(evidenceLocatorLabel(evidence("broken"))).toBe("locator 손상");
  });

  it("distinguishes valid, unknown, and definite-invalid evidence", () => {
    expect(evidenceStatusTone("valid")).toBe("text-status-done");
    expect(evidenceStatusTone("unknown")).toBe("text-status-awaiting");
    expect(evidenceStatusTone("changed")).toBe("text-status-failed");
    expect(evidenceStatusLabel("changed")).toBe("원본 변경");
    expect(evidenceStatusLabel("missing")).toBe("원본 없음");
    expect(evidenceStatusLabel("expired")).toBe("만료");
    expect(evidenceStatusLabel("unknown")).toBe("확인 불가");
  });

  it("shows backend check and expiry timestamps without local ambiguity", () => {
    expect(evidenceTimestampLabel(101)).toBe("1970-01-01T00:01:41.000Z");
    expect(evidenceTimestampLabel(null)).toBe("미검사");
  });

  it("hides stale rows while a different memory is loading", () => {
    const previous: EvidenceLoadState = {
      memoryId: 1,
      status: "ready",
      rows: [evidence('{"relative_path":"old.rs"}')],
    };

    expect(evidenceStateForMemory(previous, 2)).toEqual({
      memoryId: 2,
      status: "loading",
    });
  });

  it("does not show an action error from a previously opened memory", () => {
    expect(
      evidenceActionErrorForMemory(
        { memoryId: 1, message: "old revalidation failed" },
        2,
      ),
    ).toBeNull();
  });

  it("does not label a failed load as still checking", () => {
    expect(
      evidenceCountLabel({ memoryId: 1, status: "error", message: "offline" }),
    ).toBe("확인 실패");
  });

  it("rejects a reload owned by a previously opened memory", () => {
    expect(evidenceRequestBelongsToMemory(2, 1)).toBe(false);
    expect(evidenceRequestBelongsToMemory(2, 2)).toBe(true);
  });
});
