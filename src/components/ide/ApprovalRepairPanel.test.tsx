// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApprovalRepairPanel } from "./ApprovalRepairPanel";
import type { ApprovalRepairSession } from "../../lib/approval-readiness";

const transport = vi.hoisted(() => ({ approvalRepairStatus: vi.fn(), approvalRepairPrepare: vi.fn(), approvalRepairRun: vi.fn(), approvalRepairAccept: vi.fn(), approvalRepairCancel: vi.fn() }));
const getTransport = vi.hoisted(() => vi.fn(() => transport));
vi.mock("../../lib/transport", () => ({ getTransport, taskKey: (task: {host:string;id:number}) => `${task.host}:${task.id}` }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const prepared = (): ApprovalRepairSession => ({ id:"repair-7", task_id:7, state:"prepared", source_path:"/source", candidate_path:"/candidate", base:"dev", source_sha:"a", target_sha:"b", commands:["npm test"], attempts:0, checks:[], summary:"", error:null, diff:"", updated_at:1 });
let element: HTMLDivElement; let root: Root;
const accepted = vi.fn();
const button = (name: string) => [...element.querySelectorAll("button")].find(button => button.textContent === name)!;
const render = async (host="local") => { await act(async () => root.render(<ApprovalRepairPanel task={{host,id:7}} disabled={false} onAccepted={accepted} />)); };
/** 자동 해결은 칩 뒤 팝오버다 — 준비·시작·채택 버튼은 펼쳐야 그려진다. */
const detailChip = () => element.querySelector<HTMLButtonElement>('[aria-haspopup="dialog"]');
const openDetail = async () => { await act(async () => detailChip()?.click()); };
const renderOpen = async (host="local") => { await render(host); await openDetail(); };
const click = async (name: string) => { await act(async () => button(name).click()); };
beforeEach(() => {
  vi.clearAllMocks(); transport.approvalRepairStatus.mockResolvedValue(null); transport.approvalRepairPrepare.mockResolvedValue(prepared());
  transport.approvalRepairRun.mockResolvedValue({...prepared(),state:"ready",attempts:1,diff:"+preserve both",checks:[{command:"npm test",exit_code:0,tail:"pass"}]});
  transport.approvalRepairAccept.mockResolvedValue({...prepared(),state:"accepted"});
  element=document.createElement("div"); document.body.append(element); root=createRoot(element);
});
afterEach(() => { act(() => root.unmount()); element.remove(); });

describe("automatic approval repair", () => {
  it("previews commands before paid execution and requires review before adoption", async () => {
    await renderOpen("mini1"); await click("자동 해결 준비");
    expect(element.textContent).toContain("npm test"); expect(element.textContent).toContain("원본 보관: /source");
    expect(transport.approvalRepairRun).not.toHaveBeenCalled();
    await click("자동 해결·검사 시작");
    expect(getTransport).toHaveBeenCalledWith("mini1"); expect(transport.approvalRepairRun).toHaveBeenCalledWith(7,"repair-7");
    expect(button("해결 결과 채택").disabled).toBe(true);
    await act(async () => element.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
    await click("해결 결과 채택"); expect(accepted).toHaveBeenCalledTimes(1);
    expect(element.textContent).toContain("기존 승인");
  });
  it("does not offer adoption for failed or missing checks", async () => {
    transport.approvalRepairRun.mockResolvedValue({...prepared(),state:"needs_attention",error:"검사 실패"});
    await renderOpen(); await click("자동 해결 준비"); await click("자동 해결·검사 시작");
    expect(element.textContent).toContain("검사 실패"); expect(button("해결 결과 채택")).toBeUndefined();
    transport.approvalRepairPrepare.mockResolvedValue({...prepared(),commands:[]});
    await click("새 자동 해결 준비"); expect(button("자동 해결·검사 시작").disabled).toBe(true);
  });
  it("ignores late preparation from another host and sends a double click once", async () => {
    let finish!: (value: ApprovalRepairSession) => void;
    transport.approvalRepairPrepare.mockImplementationOnce(() => new Promise(resolve => { finish=resolve; }));
    await renderOpen();
    await act(async () => { button("자동 해결 준비").click(); button("자동 해결 준비").click(); });
    expect(transport.approvalRepairPrepare).toHaveBeenCalledTimes(1);
    await renderOpen("mini1"); await act(async () => finish(prepared()));
    expect(element.textContent).not.toContain("/candidate");
  });
  it("does not let a late status read overwrite an accepted result", async () => {
    let finish!: (value: ApprovalRepairSession) => void;
    await renderOpen(); await click("자동 해결 준비"); await click("자동 해결·검사 시작");
    transport.approvalRepairStatus.mockImplementationOnce(() => new Promise(resolve => { finish=resolve; }));
    await act(async () => window.dispatchEvent(new Event("focus")));
    await act(async () => element.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
    await click("해결 결과 채택");
    await act(async () => finish({...prepared(),state:"ready"}));
    expect(element.textContent).toContain("해결 결과 채택됨"); expect(button("해결 결과 채택")).toBeUndefined();
  });
  it("surfaces old Runner endpoint errors without starting an agent", async () => {
    transport.approvalRepairPrepare.mockRejectedValue(new Error("Runner 404"));
    await renderOpen("old-runner"); await click("자동 해결 준비");
    expect(element.textContent).toContain("Runner 404"); expect(transport.approvalRepairRun).not.toHaveBeenCalled();
  });
  it("keeps the approval bar to a chip until the detail is opened", async () => {
    transport.approvalRepairStatus.mockResolvedValue(prepared());
    await render();
    expect(element.querySelector('[role="dialog"]')).toBeNull();
    expect(element.textContent).not.toContain("/candidate");
    expect(element.querySelector('[role="status"]')?.textContent).toContain("해결 준비됨");
    await openDetail();
    expect(element.querySelector('[role="dialog"]')?.textContent).toContain("/candidate");
    await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(element.querySelector('[role="dialog"]')).toBeNull();
  });

  it("requests cancellation on the same host and session without claiming completion", async () => {
    transport.approvalRepairStatus.mockResolvedValue({...prepared(), state:"resolving"});
    await renderOpen("mini1"); await click("자동 해결 중단");
    expect(transport.approvalRepairCancel).toHaveBeenCalledWith(7,"repair-7");
    expect(element.textContent).toContain("중단 요청됨"); expect(accepted).not.toHaveBeenCalled();
  });
});
