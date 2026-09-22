import { describe, expect, it } from "vitest";
import { decodeBase64Url, describePush } from "./push";

describe("decodeBase64Url", () => {
  it("패딩 없는 base64url을 바이트로 되돌린다", () => {
    // VAPID 공개키는 65바이트 비압축 P-256 포인트이고, 패딩 없이 온다.
    expect(Array.from(decodeBase64Url("AAEC"))).toEqual([0, 1, 2]);
    expect(Array.from(decodeBase64Url("_w"))).toEqual([255]);
    expect(Array.from(decodeBase64Url("-_8"))).toEqual([251, 255]);
  });

  it("길이가 4의 배수가 아니어도 처리한다", () => {
    expect(decodeBase64Url("QQ")).toHaveLength(1);
    expect(decodeBase64Url("QUJD")).toHaveLength(3);
  });

  it("빈 문자열은 빈 배열", () => {
    expect(decodeBase64Url("")).toHaveLength(0);
  });
});

describe("describePush", () => {
  it("상태마다 무엇이 문제인지 말한다", () => {
    // "알림 꺼짐"만 보여주면 사용자는 정상인 줄 안다 — 결과를 함께 말해야 한다.
    expect(describePush({ kind: "idle" }).detail).toContain("울리지 않습니다");
    expect(describePush({ kind: "denied" }).detail).toContain("브라우저");
    expect(describePush({ kind: "unsupported", reason: "홈 화면" }).detail).toBe("홈 화면");
    expect(describePush({ kind: "error", message: "boom" }).detail).toBe("boom");
  });

  it("정상 상태에는 군더더기를 붙이지 않는다", () => {
    const view = describePush({ kind: "subscribed" });
    expect(view.label).toBe("알림 켜짐");
    expect(view.detail).toBeUndefined();
  });
});
