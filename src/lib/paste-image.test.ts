import { describe, it, expect } from "vitest";
import { findImageFile, pastedImageRef, insertAtCaret, blobToBase64 } from "./paste-image";

/** DataTransferItem 흉내 — node 환경에는 DataTransfer 생성자가 없다. */
function fakeItem(kind: string, type: string, file: File | null): DataTransferItem {
  return { kind, type, getAsFile: () => file } as unknown as DataTransferItem;
}

function fakeData(items: DataTransferItem[]): DataTransfer {
  return { items } as unknown as DataTransfer;
}

const png = new File([new Uint8Array([1, 2, 3])], "cap.png", { type: "image/png" });

describe("findImageFile", () => {
  it("이미지 파일 항목을 찾는다", () => {
    const data = fakeData([fakeItem("string", "text/plain", null), fakeItem("file", "image/png", png)]);
    expect(findImageFile(data)).toBe(png);
  });
  it("이미지가 없으면 null — 기본 텍스트 붙여넣기 유지", () => {
    expect(findImageFile(fakeData([fakeItem("string", "text/plain", null)]))).toBeNull();
    expect(findImageFile(null)).toBeNull();
  });
  it("이미지가 아닌 파일(pdf 등)은 무시한다", () => {
    const pdf = new File([new Uint8Array([1])], "doc.pdf", { type: "application/pdf" });
    expect(findImageFile(fakeData([fakeItem("file", "application/pdf", pdf)]))).toBeNull();
  });
});

describe("insertAtCaret", () => {
  it("빈 입력에는 그대로 삽입하고 caret은 끝", () => {
    expect(insertAtCaret("", 0, "[x]")).toEqual({ value: "[x]", caret: 3 });
  });
  it("단어 사이 삽입 시 앞뒤로 공백을 붙인다", () => {
    const r = insertAtCaret("ab", 1, "[x]");
    expect(r.value).toBe("a [x] b");
    expect(r.caret).toBe(6);
  });
  it("이미 공백 경계면 공백을 더하지 않는다", () => {
    expect(insertAtCaret("a ", 2, "[x]").value).toBe("a [x]");
  });
  it("범위 밖 caret은 끝으로 클램프한다", () => {
    expect(insertAtCaret("a", 99, "[x]").value).toBe("a [x]");
  });
});

describe("pastedImageRef", () => {
  it("에이전트가 읽을 경로 참조 텍스트를 만든다", () => {
    expect(pastedImageRef("/tmp/paste-1.png")).toBe("[이미지: /tmp/paste-1.png]");
  });
});

describe("blobToBase64", () => {
  it("바이트를 base64로 인코딩한다", async () => {
    const blob = new Blob([new Uint8Array([0, 1, 2, 255])]);
    expect(await blobToBase64(blob)).toBe("AAEC/w==");
  });
});
