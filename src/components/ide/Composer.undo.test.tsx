// @vitest-environment jsdom

import { act, useEffect, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { initialInterviewState } from "../../lib/interview";
import { initialGrillState } from "../../lib/grill";
import type { HostId } from "../../lib/transport";

const mocks = vi.hoisted(() => ({
  gitStatus: vi.fn(async () => true),
  useWorktreeOverrideGet: vi.fn(async (): Promise<boolean | null> => null),
  useWorktreeOverrideClear: vi.fn(async () => {}),
  useWorktreeSet: vi.fn(async () => {}),
}));

vi.mock("../../lib/ipc", () => ({
  skillsList: vi.fn(async () => []),
  fsTreePath: vi.fn(async () => []),
  flattenFiles: vi.fn(() => []),
  gitStatus: mocks.gitStatus,
  gitInit: vi.fn(async () => true),
  gitBranches: vi.fn(async () => ({ current: "main", branches: ["main"] })),
  pasteImageSave: vi.fn(async () => ""),
  useWorktreeOverrideGet: mocks.useWorktreeOverrideGet,
  useWorktreeOverrideClear: mocks.useWorktreeOverrideClear,
  useWorktreeSet: mocks.useWorktreeSet,
}));
vi.mock("../../lib/transport", () => ({
  LOCAL_HOST: "local",
  getTransport: vi.fn(() => ({ kind: "local" })),
  hasHost: vi.fn(() => true),
  listHosts: vi.fn(() => ["local"]),
}));
vi.mock("../../lib/transport/runner", () => ({
  RunnerTransport: class RunnerTransport {},
}));
vi.mock("./RepoPicker", () => ({ RepoPicker: () => null }));
vi.mock("./DirectoryPickerModal", () => ({ DirectoryPickerModal: () => null }));
vi.mock("./AgentPicker", () => ({ AgentPicker: () => null }));
vi.mock("./ModelPicker", () => ({ ModelPicker: () => null }));
vi.mock("./EffortPicker", () => ({ EffortPicker: () => null }));
vi.mock("./MentionDropdown", () => ({ MentionDropdown: () => null }));
vi.mock("./SkillDropdown", () => ({ SkillDropdown: () => null }));
vi.mock("./InterviewPanel", () => ({ InterviewPanel: () => null }));
vi.mock("./icons", () => ({ Icon: () => null }));

import { Composer } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

/** 래퍼가 밖으로 꺼내 주는 setInstruction — 훅을 거치지 않은 외부 값 변경을 흉내 낸다. */
const external: {
  set: ((next: string) => void) | null;
  switchHost: ((next: HostId, draft: string) => void) | null;
} = { set: null, switchHost: null };

beforeEach(() => {
  external.set = null;
  external.switchHost = null;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.useRealTimers();
  vi.clearAllMocks();
});

/** instruction을 스스로 들고 있는 래퍼 — 되돌리기는 controlled 값이 실제로 되돌아가야 성립한다. */
function Harness({ initial = "", busy = false }: { initial?: string; busy?: boolean }) {
  const [instruction, setInstruction] = useState(initial);
  const [host, setHost] = useState<HostId>("local");
  useEffect(() => {
    external.set = setInstruction;
    // 호스트 전환은 App이 한 핸들러에서 호스트와 지시문을 함께 바꾼다 — 한 커밋이다.
    external.switchHost = (next, draft) => {
      setHost(next);
      setInstruction(draft);
    };
  }, []);
  return (
    <Composer
      host={host}
      setHost={() => undefined}
      repo="/repo"
      setRepo={() => undefined}
      recentRepos={[]}
      agents={["codex"]}
      setAgents={() => undefined}
      resumeSession={null}
      onResumeSessionChange={() => undefined}
      model=""
      setModel={() => undefined}
      reasoningEffort=""
      setReasoningEffort={() => undefined}
      instruction={instruction}
      setInstruction={setInstruction}
      interview={initialInterviewState()}
      onInterviewStart={() => undefined}
      onInterviewAnswer={() => undefined}
      onInterviewCrystallize={() => undefined}
      onInterviewRetry={() => undefined}
      grill={initialGrillState()}
      onGrillStart={() => undefined}
      onGrillDraft={() => undefined}
      onGrillAnswer={() => undefined}
      onGrillAcceptRecommendation={() => undefined}
      onGrillDontKnow={() => undefined}
      onGrillEndNow={() => undefined}
      onGrillApplyAndScore={() => undefined}
      onGrillApplyInstruction={() => undefined}
      onGrillSave={() => undefined}
      onGrillRetry={() => undefined}
      useWorktree
      baseBranch=""
      setBaseBranch={() => undefined}
      busy={busy}
      creating={null}
      onCreate={() => undefined}
    />
  );
}

async function renderHarness(opts?: { initial?: string; busy?: boolean }): Promise<void> {
  await act(async () => {
    root?.render(<Harness initial={opts?.initial} busy={opts?.busy} />);
  });
}

function instructionField(): HTMLTextAreaElement {
  const field = container?.querySelector<HTMLTextAreaElement>(
    'textarea[placeholder^="작업을 설명하세요"]',
  );
  if (!field) throw new Error("작업 지시 입력창을 찾을 수 없습니다");
  return field;
}

const nativeValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;

/** controlled textarea에 사용자 입력을 흉내 낸다 — React value tracker를 우회해야 onChange가 뜬다. */
async function typeValue(next: string): Promise<void> {
  const field = instructionField();
  await act(async () => {
    nativeValue.call(field, next);
    field.setSelectionRange(next.length, next.length);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

/** 한 음절의 IME 조합 — compositionstart → 자모 단위 input → compositionend. */
async function composeSyllable(steps: string[]): Promise<void> {
  const field = instructionField();
  await act(async () => {
    field.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
  });
  for (const step of steps) await typeValue(step);
  await act(async () => {
    field.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
  });
}

async function press(init: KeyboardEventInit): Promise<KeyboardEvent> {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  await act(async () => {
    instructionField().dispatchEvent(event);
  });
  return event;
}

/** 연속 타이핑 묶기(800ms)를 시간으로 끊기 위해 Date만 고정한다 — React 스케줄러는 건드리지 않는다. */
function useFakeClock(): void {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(0);
}

function advance(ms: number): void {
  vi.setSystemTime(Date.now() + ms);
}

describe("Composer 되돌리기/다시하기", () => {
  it("타이핑 묶음이 끊긴 뒤 ⌘Z는 직전 묶음으로, ⇧⌘Z는 다시 앞으로 간다", async () => {
    useFakeClock();
    await renderHarness();
    await typeValue("가");
    advance(2000);
    await typeValue("가나");

    await press({ key: "z", metaKey: true });
    expect(instructionField().value).toBe("가");

    await press({ key: "z", metaKey: true, shiftKey: true });
    expect(instructionField().value).toBe("가나");
  });

  it("Ctrl+Z·⇧Ctrl+Z도 같은 되돌리기·다시하기를 한다", async () => {
    useFakeClock();
    await renderHarness();
    await typeValue("가");
    advance(2000);
    await typeValue("가나");

    await press({ key: "z", ctrlKey: true });
    expect(instructionField().value).toBe("가");

    await press({ key: "z", ctrlKey: true, shiftKey: true });
    expect(instructionField().value).toBe("가나");
  });

  it("한 번에 들어온 여러 글자는 연속 타이핑과 별개 단계로 되돌아간다", async () => {
    await renderHarness();
    // 두 글자는 연속 타이핑이라 한 묶음, 그다음 붙여넣기는 그 자체로 한 단계다.
    await typeValue("가");
    await typeValue("가나");
    await typeValue("가나붙여넣기");

    await press({ key: "z", metaKey: true });
    expect(instructionField().value).toBe("가나");

    await press({ key: "z", metaKey: true });
    expect(instructionField().value).toBe("");
  });

  it("부모가 밖에서 비운 값도 ⌘Z로 되돌아온다", async () => {
    await renderHarness({ initial: "제출할 초안" });

    await act(async () => external.set?.(""));
    expect(instructionField().value).toBe("");

    await press({ key: "z", metaKey: true });
    expect(instructionField().value).toBe("제출할 초안");
  });

  it("busy(readOnly)일 때 ⌘Z는 값을 바꾸지 않는다", async () => {
    await renderHarness({ initial: "제출할 초안", busy: true });
    await act(async () => external.set?.(""));

    await press({ key: "z", metaKey: true });

    expect(instructionField().value).toBe("");
  });

  it("되돌리기 키는 브라우저 기본 동작을 막는다", async () => {
    await renderHarness();

    const event = await press({ key: "z", metaKey: true });

    expect(event.defaultPrevented).toBe(true);
  });

  it("일반 문자 키는 막지 않는다", async () => {
    await renderHarness();

    const event = await press({ key: "z" });

    expect(event.defaultPrevented).toBe(false);
  });

  it("호스트를 바꾸면 앞 호스트의 문장이 ⌘Z로 새어 들지 않는다", async () => {
    await renderHarness();
    await typeValue("로컬에 쓰던 문장");

    await act(async () => external.switchHost?.("remote" as HostId, ""));
    expect(instructionField().value).toBe("");

    await press({ key: "z", metaKey: true });
    expect(instructionField().value).toBe("");
  });

  it("IME 조합은 자모가 아니라 한 단어로 되돌아간다", async () => {
    await renderHarness();

    await composeSyllable(["ㅎ", "하", "한"]);
    await composeSyllable(["한ㄱ", "한그", "한글"]);

    await press({ key: "z", metaKey: true });
    expect(instructionField().value).toBe("");
  });
});
