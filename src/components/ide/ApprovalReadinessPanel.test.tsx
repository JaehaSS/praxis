// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApprovalReadinessPanel } from "./ApprovalReadinessPanel";
import { taskApprovalStatus } from "../../lib/ipc";
import type { ApprovalStatus } from "../../lib/approval-readiness";

vi.mock("../../lib/ipc", () => ({ taskApprovalStatus: vi.fn() }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ready = (): ApprovalStatus => ({
  readiness: { base: "dev", source_sha: "a", target_sha: "b", observed_at: 100, source_changes: 0, already_integrated: false, direct: false, remote_ahead: 0, remote_behind: 0, issues: [] },
  inspection_error: null, attempts: [],
});
const rejected = (): ApprovalStatus => ({ ...ready(), attempts: [{ attempt_id: 2, task_id: 7, ts: 100, stage: "commit", outcome: "failed", base: "dev", source_sha: "a", target_sha: "b", direct: false, error: "문서 번호 중복\nERROR docs/notes.md" }] });
let el: HTMLDivElement;
let root: Root;
const repair = vi.fn<(...args: [string]) => Promise<boolean>>();
const resolve = vi.fn();
const button = (label: string) => [...el.querySelectorAll("button")].find((b) => b.textContent?.includes(label));
async function render(host = "local", refreshKey = "0") {
  await act(async () => root.render(<ApprovalReadinessPanel task={{ host, id: 7 }} base="dev" refreshKey={refreshKey} disabled={false} onRepair={repair} onResolve={host === "local" ? resolve : undefined} />));
}
/** 상세는 접힌 채로 시작한다 — 칩을 눌러야 팝오버가 그려진다. */
const detailChip = () => el.querySelector<HTMLButtonElement>('[aria-haspopup="dialog"]');
async function openDetail() { await act(async () => detailChip()?.click()); }
/** 대부분의 검사는 펼친 상태를 본다. 접힘 자체는 아래 전용 검사가 지킨다. */
async function renderOpen(host = "local", refreshKey = "0") { await render(host, refreshKey); await openDetail(); }
async function click(label: string) { await act(async () => button(label)?.click()); }

beforeEach(() => {
  vi.resetAllMocks(); repair.mockResolvedValue(true);
  vi.mocked(taskApprovalStatus).mockResolvedValue(ready());
  el = document.createElement("div"); document.body.append(el); root = createRoot(el);
});
afterEach(() => { act(() => root.unmount()); el.remove(); });

describe("Approval readiness", () => {
  it("shows target blocker paths and keeps pending changes distinct from checked HEAD", async () => {
    const status = ready();
    status.readiness!.source_changes = 3;
    status.readiness!.issues = [{ code: "target_dirty", message: "대상 폴더에 변경 1건이 있습니다", location: "/repo", paths: [{ status: "??", path: "notes with\nnewline.md" }], total_paths: 1 }];
    vi.mocked(taskApprovalStatus).mockResolvedValue(status);
    await renderOpen();
    expect(el.textContent).toContain("준비 필요"); expect(el.textContent).toContain("notes with\nnewline.md");
    status.readiness!.issues = [];
    await click("다시 확인");
    expect(el.textContent).toContain("작업 변경 3건"); expect(el.textContent).not.toContain("사전 점검 통과");
  });

  it("sends original commit failure and scope to the same task without an input draft", async () => {
    vi.mocked(taskApprovalStatus).mockResolvedValue(rejected());
    await renderOpen("mini1");
    expect(el.querySelector('[role="status"]')?.textContent).toContain("최근 승인 실패 · 자동 커밋·훅");
    expect(el.textContent).not.toContain("사전 점검 통과");
    await click("문서·커밋 오류 수정 요청");
    expect(taskApprovalStatus).toHaveBeenCalledWith({ host: "mini1", id: 7 });
    expect(repair).toHaveBeenCalledTimes(1);
    expect(repair.mock.calls[0][0]).toContain('"host": "mini1"');
    expect(repair.mock.calls[0][0]).toContain('"task_id": 7');
    expect(repair.mock.calls[0][0]).toContain("문서 번호 중복");
    expect(repair.mock.calls[0][0]).toContain("대상 체크아웃은 보존");
    expect(el.textContent).toContain("수정 요청 전달됨");
    expect(el.querySelector("textarea")).toBeNull();
  });

  it("allows a failed message delivery to be retried", async () => {
    vi.mocked(taskApprovalStatus).mockResolvedValue(rejected());
    repair.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    await renderOpen(); await click("문서·커밋 오류 수정 요청");
    expect(el.textContent).toContain("수정 요청을 보내지 못했습니다");
    await click("문서·커밋 오류 수정 요청"); expect(repair).toHaveBeenCalledTimes(2);
  });

  it("rejects old-host responses when numeric task IDs coincide", async () => {
    let oldResolve!: (value: ApprovalStatus) => void;
    vi.mocked(taskApprovalStatus).mockImplementationOnce(() => new Promise((done) => { oldResolve = done; }));
    await renderOpen("local"); await renderOpen("mini1");
    await act(async () => oldResolve(rejected()));
    expect(el.textContent).not.toContain("문서 번호 중복");
    expect(el.textContent).toContain("사전 점검 통과");
  });

  it("shows unknown on unavailable endpoints and never leaves an old pass visible", async () => {
    await renderOpen();
    vi.mocked(taskApprovalStatus).mockRejectedValue(new Error("Runner 404"));
    await click("다시 확인");
    expect(el.textContent).toContain("준비 상태 확인 불가");
    expect(el.textContent).not.toContain("사전 점검 통과");
  });

  it("connects local conflicts to the existing resolver without using local IPC for remote tasks", async () => {
    const status = ready(); status.readiness!.issues = [{ code: "merge_conflict", message: "코드 충돌", location: "/task", total_paths: 1, paths: [{ status: "UU", path: "queue.rs" }] }];
    vi.mocked(taskApprovalStatus).mockResolvedValue(status);
    await renderOpen(); await click("충돌 확인·해소"); expect(resolve).toHaveBeenCalledTimes(1);
    await renderOpen("mini1"); expect(button("충돌 확인·해소")).toBeUndefined();
  });

  it("focus reinspection does not strand an in-flight repair failure", async () => {
    let complete!: (value: boolean) => void;
    repair.mockImplementationOnce(() => new Promise((done) => { complete = done; }));
    vi.mocked(taskApprovalStatus).mockResolvedValue(rejected());
    await renderOpen(); await click("문서·커밋 오류 수정 요청");
    await act(async () => window.dispatchEvent(new Event("focus")));
    await act(async () => complete(false));
    expect(button("문서·커밋 오류 수정 요청")?.disabled).toBe(false);
    expect(el.textContent).toContain("수정 요청을 보내지 못했습니다");
  });

  it("keeps an unfinished attempt distinct from a passed inspection", async () => {
    const status = rejected(); status.attempts[0].outcome = "started"; status.attempts[0].error = null;
    vi.mocked(taskApprovalStatus).mockResolvedValue(status);
    await renderOpen();
    expect(el.textContent).toContain("최근 승인 결과 미확인");
    expect(el.textContent).not.toContain("사전 점검 통과");
    status.attempts[0].outcome = "succeeded";
    await click("다시 확인");
    expect(el.textContent).toContain("사전 점검 통과");
  });

  it("keeps the detail collapsed until asked, even when the inspection needs attention", async () => {
    const status = ready();
    status.readiness!.issues = [{ code: "target_dirty", message: "대상 폴더에 변경 1건이 있습니다", location: "/repo", paths: [], total_paths: 0 }];
    vi.mocked(taskApprovalStatus).mockResolvedValue(status);
    await render();
    expect(el.querySelector('[role="dialog"]')).toBeNull();
    expect(el.textContent).not.toContain("대상 폴더에 변경 1건이 있습니다");
    expect(el.querySelector('[role="status"]')?.textContent).toContain("준비 필요");
    await openDetail();
    expect(el.querySelector('[role="dialog"]')?.textContent).toContain("대상 폴더에 변경 1건이 있습니다");
  });

  it("keeps the opened detail across conversation updates and closes it on another task", async () => {
    await renderOpen();
    expect(el.querySelector('[role="dialog"]')).not.toBeNull();
    await render("local", "1");
    expect(el.querySelector('[role="dialog"]')).not.toBeNull();
    await render("mini1");
    expect(el.querySelector('[role="dialog"]')).toBeNull();
  });

  it("closes the detail on Escape", async () => {
    await renderOpen();
    await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(el.querySelector('[role="dialog"]')).toBeNull();
  });

  it("sends only once when repair is clicked twice before rerender", async () => {
    vi.mocked(taskApprovalStatus).mockResolvedValue(rejected());
    await renderOpen();
    await act(async () => {
      const repairButton = button("문서·커밋 오류 수정 요청");
      repairButton?.click(); repairButton?.click();
    });
    expect(repair).toHaveBeenCalledTimes(1);
  });
});
