// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { QuestionReference } from "../../lib/side-question";
import { QuestionReferenceAttachments } from "./QuestionReferenceAttachments";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
const reference: QuestionReference = {
  id: "r1",
  sourceKey: "local:1",
  question: "왜 재시도하나요?",
  text: "네트워크 복구를 위해서입니다.",
  contexts: [{ label: "retry.ts", text: "retry()", path: "src/retry.ts" }],
  incomplete: false,
};

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe("QuestionReferenceAttachments", () => {
  it("edits an immutable main-draft copy and can remove it", async () => {
    const onChange = vi.fn();
    await act(async () => root.render(<QuestionReferenceAttachments references={[reference]} onChange={onChange} />));

    await act(async () => (host.querySelector('[aria-label="참고자료 편집"]') as HTMLButtonElement).click());
    const field = host.querySelector('textarea[id^="question-reference-"]') as HTMLTextAreaElement;
    await act(async () => {
      Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")?.set?.call(field, "사용자가 고른 편집본");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => (Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "저장") as HTMLButtonElement).click());

    expect(onChange).toHaveBeenCalledWith([{ ...reference, text: "사용자가 고른 편집본" }]);
    expect(reference.text).toBe("네트워크 복구를 위해서입니다.");

    await act(async () => (host.querySelector('[aria-label="참고자료 제거"]') as HTMLButtonElement).click());
    expect(onChange).toHaveBeenLastCalledWith([]);
  });
});
