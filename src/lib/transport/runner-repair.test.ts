import { describe, expect, it, vi } from "vitest";
import { RunnerTransport } from "./runner";

const token = "ab".repeat(32);

describe("Runner review process repair transport", () => {
  it("lists quarantines and reconciles the selected receipt with bearer auth", async () => {
    const request = vi.fn(async (url: RequestInfo | URL, _init?: RequestInit) => {
      if (String(url).endsWith("/quarantined")) {
        return json([
          {
            receipt_id: 17,
            task_id: 7,
            operation: "verify",
            phase: "verify_test",
            pgid: 401,
            state: "quarantined",
            reason: "verification_uncertain",
            detail: "확인할 수 없습니다.",
            created_at: 1,
            updated_at: 2,
          },
        ]);
      }
      return json({ receipt_id: 17, status: "resolved_absent", reason: null, detail: null });
    });
    const transport = new RunnerTransport(
      {
        endpoint: "http://127.0.0.1:49123/",
        pairingToken: token,
        profileName: "production",
      },
      request,
    );

    expect(transport.profileName()).toBe("production");
    await expect(transport.reviewProcessQuarantines()).resolves.toHaveLength(1);
    await expect(transport.reviewProcessReconcile(17)).resolves.toMatchObject({
      status: "resolved_absent",
    });

    expect(request.mock.calls.map(([url]) => String(url))).toEqual([
      "http://127.0.0.1:49123/v1/review-processes/quarantined",
      "http://127.0.0.1:49123/v1/review-processes/17/reconcile",
    ]);
    expect(request.mock.calls[1][1]).toMatchObject({
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    });
  });
});

function json(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}
