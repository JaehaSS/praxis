import { describe, expect, it } from "vitest";
import { UNDO_SIZE_LIMIT, planAutosave } from "./autosave";
import type { OpenFile } from "../components/ide/EditorPane";
import { fileTabKey } from "./tab-key";

const file = (over: Partial<OpenFile> = {}): OpenFile => ({
  key: fileTabKey(over.path ?? "src/a.ts"),
  path: "src/a.ts",
  kind: "text",
  content: "my-edit",
  baseContent: "agent-made",
  mtime: 1,
  dirty: true,
  ...over,
});

describe("planAutosave", () => {
  it("dirty가 아니면 아무것도 하지 않는다", () => {
    expect(planAutosave(file({ dirty: false }), "agent-made")).toEqual({ kind: "skip" });
  });

  it("디스크가 baseContent 그대로면 저장하고 그 내용을 되돌리기 버퍼로 쓴다", () => {
    // baseContent가 곧 "에이전트가 만든 마지막 상태"다 — 되돌린다는 것은 여기로 가는 것이다.
    const plan = planAutosave(file(), "agent-made");
    expect(plan).toEqual({ kind: "save", content: "my-edit", undoContent: "agent-made" });
  });

  it("디스크가 그사이 바뀌었으면 저장하지 않는다", () => {
    // 에이전트가 같은 파일을 고쳤다는 뜻이다. 덮어쓰면 그 작업이 사라진다.
    const plan = planAutosave(file(), "agent-changed-it");
    expect(plan).toEqual({ kind: "conflict", path: "src/a.ts" });
  });

  it("편집값이 디스크와 같아지면 충돌이 아니다", () => {
    // 되돌려 놓았거나 우연히 같아진 경우 — 쓸 것이 없으니 막을 이유도 없다.
    const plan = planAutosave(file({ content: "agent-made" }), "agent-made");
    expect(plan).toEqual({ kind: "save", content: "agent-made", undoContent: "agent-made" });
  });

  it("상한을 넘는 파일은 저장하되 되돌리기를 제공하지 않는다", () => {
    const big = "x".repeat(UNDO_SIZE_LIMIT + 1);
    const plan = planAutosave(file({ baseContent: big }), big);
    expect(plan).toEqual({ kind: "save", content: "my-edit", undoContent: null });
  });

  it("텍스트가 아닌 파일은 손대지 않는다", () => {
    // 이미지·바이너리는 Monaco가 편집하지 않으므로 dirty가 될 일이 없지만,
    // 만에 하나 표시가 어긋나도 디스크에 쓰지는 않는다.
    expect(planAutosave(file({ kind: "image" }), "agent-made")).toEqual({ kind: "skip" });
  });
});
