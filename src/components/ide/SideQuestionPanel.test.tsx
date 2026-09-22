// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { contextSourceHash, type SideQuestionApi, type SideQuestionInput, type SideQuestionSnapshot } from "../../lib/side-question";
import { SideQuestionPanel } from "./SideQuestionPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const snapshot = (turns: SideQuestionSnapshot["turns"] = []): SideQuestionSnapshot => ({
  task_id: 12,
  thread_id: 9,
  generation: 3,
  model: "gpt-test",
  supported: true,
  reason: null,
  turns,
});

let host: HTMLDivElement;
let root: Root;
let api: SideQuestionApi;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  api = { read: vi.fn(async () => snapshot()), send: vi.fn(async () => snapshot()), cancel: vi.fn(async () => snapshot()), reset: vi.fn(async () => snapshot()) };
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function render(options?: Partial<React.ComponentProps<typeof SideQuestionPanel>>) {
  await act(async () => {
    root.render(<SideQuestionPanel sessionKey="local:12" api={api} active onAttach={() => undefined} onBack={() => undefined} {...options} />);
  });
}

function field(): HTMLTextAreaElement {
  return host.querySelector("textarea#side-question-input") as HTMLTextAreaElement;
}

function button(label: string): HTMLButtonElement {
  const found = Array.from(host.querySelectorAll("button")).find((item) => item.textContent === label);
  if (!found) throw new Error(`button not found: ${label}`);
  return found;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function typeQuestion(text: string): Promise<void> {
  await act(async () => {
    const input = field();
    Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")?.set?.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("SideQuestionPanel", () => {
  it("sends only explicit draft material and clears it after the accepted snapshot", async () => {
    await render({ initialContext: { id: "selection:1", context: { label: "api.ts · 선택", text: "const retry = true", path: "src/api.ts" } } });
    await typeQuestion("중복 실행될까?");
    await act(async () => button("질문 보내기").click());

    expect(api.send).toHaveBeenCalledOnce();
    expect(api.send).toHaveBeenCalledWith(expect.objectContaining({ generation: 3, question: "중복 실행될까?", contexts: [{ label: "api.ts · 선택", text: "const retry = true", path: "src/api.ts" }] }));
    expect(field().value).toBe("");
    expect(host.textContent).toContain("이 질의의 대화만 참고합니다");
  });

  it("does not send while IME composition owns Enter", async () => {
    await render();
    await typeQuestion("한글 질문");
    await act(async () => {
      const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
      Object.defineProperty(event, "isComposing", { value: true });
      field().dispatchEvent(event);
    });
    expect(api.send).not.toHaveBeenCalled();
  });

  it("keeps an uncertain request and reconciles its original request id before clearing the sent draft", async () => {
    const sent = vi.fn(async (_input: SideQuestionInput): Promise<SideQuestionSnapshot> => { throw new Error("response lost"); });
    api = { ...api, send: sent };
    await render();
    await typeQuestion("응답 유실 뒤 중복될까?");
    await act(async () => {
      button("질문 보내기").click();
    });
    expect(field().value).toBe("응답 유실 뒤 중복될까?");
    expect(host.textContent).toContain("연결 후 전송 상태를 확인합니다.");
    const input = sent.mock.calls[0][0];
    api = {
      ...api,
      read: vi.fn(async () => snapshot([{
        id: 21,
        request_id: input.request_id,
        generation: input.generation,
        question: input.question,
        contexts: input.contexts,
        answer: "접수됨",
        state: "queued",
        error: null,
        created_at: 1,
      }])),
    };
    await render();
    await act(async () => button("상태 확인").click());
    expect(field().value).toBe("");
    expect(api.send).toHaveBeenCalledOnce();
  });

  it("does not mint a second request after an uncertain delivery and retries the exact pending input on demand", async () => {
    const sent = vi.fn(async (_input: SideQuestionInput): Promise<SideQuestionSnapshot> => { throw new Error("response lost"); });
    api = { ...api, send: sent };
    await render({ sessionKey: "local:retry" });
    await typeQuestion("같은 요청으로 복구할까?");
    await act(async () => button("질문 보내기").click());
    await act(async () => button("상태 확인").click());

    expect(field().value).toBe("같은 요청으로 복구할까?");
    expect(button("같은 요청 다시 시도")).toBeTruthy();

    sent.mockResolvedValueOnce(snapshot());
    await act(async () => button("같은 요청 다시 시도").click());

    expect(sent).toHaveBeenCalledTimes(2);
    expect(sent.mock.calls[1][0]).toEqual(sent.mock.calls[0][0]);
    expect(field().value).toBe("");
  });

  it("synchronously blocks a second click while the first send is pending", async () => {
    const response = deferred<SideQuestionSnapshot>();
    const sent = vi.fn((_input: SideQuestionInput) => response.promise);
    api = { ...api, send: sent };
    await render({ sessionKey: "local:double-click" });
    await typeQuestion("두 번 누르면 안 돼");
    await act(async () => {
      button("질문 보내기").click();
      button("질문 보내기").click();
    });
    expect(sent).toHaveBeenCalledOnce();
    response.resolve(snapshot());
    await act(async () => { await response.promise; });
  });

  it("does not enable a new question while this side thread has active work", async () => {
    api = {
      ...api,
      read: vi.fn(async () => snapshot([{
        id: 31,
        request_id: "busy-request",
        generation: 3,
        question: "진행 중",
        contexts: [],
        answer: "",
        state: "running",
        error: null,
        created_at: 1,
      }])),
    };
    await render({ sessionKey: "local:busy" });
    await typeQuestion("새 질문");
    expect(button("질문 보내기").disabled).toBe(true);
    await act(async () => button("질문 보내기").click());
    expect(api.send).not.toHaveBeenCalled();
  });

  it("drops a late file read from a previous session instead of appending it to the new session", async () => {
    const file = deferred<string>();
    await render({ sessionKey: "local:file-a", files: ["src/a.ts"], readFile: () => file.promise });
    await act(async () => {
      const select = host.querySelector("select#side-question-file") as HTMLSelectElement;
      Object.getOwnPropertyDescriptor(window.HTMLSelectElement.prototype, "value")?.set?.call(select, "src/a.ts");
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => button("추가").click());
    await render({ sessionKey: "local:file-b", files: [] });
    file.resolve("const belongsToA = true;");
    await act(async () => { await file.promise; });

    expect(host.textContent).not.toContain("a.ts");
    expect(host.textContent).not.toContain("belongsToA");
  });

  it("detects append-only changes to a complete-file capture without mutating the captured text", async () => {
    await render({
      sessionKey: "local:source-change",
      initialContext: {
        id: "source:1",
        context: {
          label: "api.ts",
          text: "const old = true",
          path: "src/api.ts",
          source_hash: contextSourceHash("const old = true"),
        },
      },
      readFile: async () => "const old = true\nconst appended = true",
    });
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });

    expect(host.textContent).toContain("파일 이후 변경됨");
    expect(host.querySelector('[title="const old = true"]')).toBeTruthy();
  });

  it("shows an explicit unavailable state when a captured source can no longer be read", async () => {
    await render({
      sessionKey: "local:source-unavailable",
      initialContext: { id: "source:2", context: { label: "missing.ts", text: "const old = true", path: "src/missing.ts" } },
      readFile: async () => { throw new Error("missing"); },
    });
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });

    expect(host.textContent).toContain("파일 상태를 확인할 수 없음");
  });

  it("checks visible turn contexts as well as the unsent draft", async () => {
    api = {
      ...api,
      read: vi.fn(async () => snapshot([{
        id: 38,
        request_id: "source-turn",
        generation: 3,
        question: "파일을 검토해줘",
        contexts: [{
          label: "full.ts",
          text: "export const current = 1;",
          path: "src/full.ts",
          source_hash: contextSourceHash("export const current = 1;"),
        }],
        answer: "확인했습니다.",
        state: "completed",
        error: null,
        created_at: 1,
      }])),
    };
    await render({ sessionKey: "local:turn-source", readFile: async () => "export const current = 1;\nexport const later = 2;" });
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });

    expect(host.textContent).toContain("파일 이후 변경됨");
    expect(host.textContent).toContain("full.ts");
  });

  it("focuses the answer editor and closes it with Escape", async () => {
    api = {
      ...api,
      read: vi.fn(async () => snapshot([{
        id: 41,
        request_id: "completed-request",
        generation: 3,
        question: "답변 선택",
        contexts: [],
        answer: "선택할 답변",
        state: "completed",
        error: null,
        created_at: 1,
      }])),
    };
    await render({ sessionKey: "local:answer-edit" });
    const outsideAnswer = Array.from(host.querySelectorAll("p")).find((item) => item.textContent === "답변 선택");
    const selection = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(outsideAnswer!);
    selection?.removeAllRanges();
    selection?.addRange(range);
    await act(async () => {
      button("참고자료로 선택").click();
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });
    const editor = host.querySelector('textarea[aria-label="메인에 첨부할 답변"]') as HTMLTextAreaElement;
    expect(document.activeElement).toBe(editor);
    expect(editor.value).toBe("선택할 답변");
    await act(async () => editor.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
    expect(host.querySelector('textarea[aria-label="메인에 첨부할 답변"]')).toBeNull();
  });

  it("preserves a session draft across a panel remount", async () => {
    await render();
    await typeQuestion("남겨 둔 질문");
    await act(async () => {
      root.render(<SideQuestionPanel sessionKey="local:12" api={api} active={false} onAttach={() => undefined} onBack={() => undefined} />);
    });
    await render();
    expect(field().value).toBe("남겨 둔 질문");
  });

  it("renders no transcript or controls while inactive", async () => {
    await render({ active: false });
    expect(host.innerHTML).toBe("");
    expect(api.read).not.toHaveBeenCalled();
  });
});
