// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { QuestionCard, QuestionSession } from "./QuestionSession";
import { answerQuestion, interactionSnapshot, questionReceipt, saveQuestionDraft, type Interaction } from "../../lib/conversation-interaction";
import { appendConvoEvent, coalesceConvoEvent, type ConvoEventLike } from "./ConversationView";
vi.mock("@tauri-apps/api/event",()=>({listen:vi.fn(async()=>()=>{})}));
vi.mock("../../lib/conversation-interaction",async(importOriginal)=>({
  ...await importOriginal<typeof import("../../lib/conversation-interaction")>(),
  answerQuestion:vi.fn(),questionReceipt:vi.fn(),saveQuestionDraft:vi.fn(),interactionSnapshot:vi.fn(),retryQuestionCleanup:vi.fn(),
}));
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;
let container:HTMLDivElement;let root:Root;
const refresh=vi.fn(async()=>{});
const item=():Interaction=>({id:"question-one",execution_id:"execution-one",call_id:"call-one",questions:{kind:"clarification",questions:[{id:"color",question:"어떤 색상?",options:[{id:"blue",label:"파랑",description:""}],allow_free_text:true,is_secret:false}]},state:"pending",reason:null,revision:1,expires_at:Date.now()/1000+100,draft:[],draft_revision:0,receipt:null});
beforeEach(()=>{vi.clearAllMocks();container=document.createElement("div");document.body.append(container);root=createRoot(container);vi.mocked(saveQuestionDraft).mockResolvedValue(1);vi.mocked(answerQuestion).mockImplementation(async(_task,_item,id)=>({request_id:id,state:"claimed"}));});
afterEach(async()=>{await act(async()=>root.unmount());container.remove();});
async function card(value=item()){await act(async()=>root.render(<QuestionCard key={value.id} taskId={7} item={value} phase="running" refresh={refresh}/>));}
async function choose(){await act(async()=>{(container.querySelector('input[type="radio"]') as HTMLInputElement).click();});}
function button(label:string){return [...container.querySelectorAll("button")].find((b)=>b.textContent===label)!;}
it("persists a question draft separately, then submits a single immutable receipt on double click",async()=>{
  await card();await choose();expect(saveQuestionDraft).toHaveBeenCalledWith(7,expect.objectContaining({id:"question-one"}),[{question_id:"color",option_id:"blue",text:null}],0);
  let finish!:(receipt:{request_id:string;state:string})=>void;
  vi.mocked(answerQuestion).mockImplementation((_task,_item,id)=>new Promise((resolve)=>{finish=(r)=>resolve({...r,request_id:id});}));
  await act(async()=>{button("답변 보내기").click();button("답변 보내기").click();});
  expect(answerQuestion).toHaveBeenCalledTimes(1);
  await act(async()=>finish({request_id:"id",state:"claimed"}));
  expect(container.textContent).toContain("답변 접수됨");expect(container.querySelector("fieldset")?.disabled).toBe(true);
});
it("queries an uncertain receipt without resending the answer",async()=>{
  await card();await choose();vi.mocked(answerQuestion).mockRejectedValue(new Error("connection lost"));
  await act(async()=>button("답변 보내기").click());
  expect(container.querySelector("fieldset")?.disabled).toBe(true);
  const id=vi.mocked(answerQuestion).mock.calls[0][2];vi.mocked(questionReceipt).mockResolvedValue({request_id:id,state:"written"});
  await act(async()=>button("접수 상태 확인").click());
  expect(questionReceipt).toHaveBeenCalledWith(7,id);expect(answerQuestion).toHaveBeenCalledTimes(1);expect(container.textContent).toContain("답변 전송됨");
});
it("IME composition does not submit and a normal modifier Enter targets this question only",async()=>{
  const value=item();value.draft=[{question_id:"color",option_id:null,text:"직접 답변"}];await card(value);
  const input=container.querySelector("textarea")!;
  await act(async()=>{input.dispatchEvent(new KeyboardEvent("keydown",{bubbles:true,key:"Enter",ctrlKey:true,isComposing:true}));});expect(answerQuestion).not.toHaveBeenCalled();
  await act(async()=>{input.dispatchEvent(new KeyboardEvent("keydown",{bubbles:true,key:"Enter",ctrlKey:true}));});
  expect(answerQuestion).toHaveBeenCalledWith(7,expect.objectContaining({id:value.id,execution_id:value.execution_id}),expect.any(String),value.draft);
});
it("restores a submitted answer as read-only after returning to the task",async()=>{
  const value=item();value.draft=[{question_id:"color",option_id:"blue",text:null}];value.receipt={request_id:"old",state:"acknowledged"};value.state="closed";value.reason="answered";
  await card(value);expect((container.querySelector("input") as HTMLInputElement).checked).toBe(true);expect(container.textContent).toContain("도구 응답 반영됨");expect(button("답변 보내기")).toBeUndefined();
});
it("remote task views do not read local question IPC",async()=>{
  await act(async()=>root.render(<QuestionSession taskId={null} linkedIds={[]}>{(_render,status)=><div>{status}</div>}</QuestionSession>));expect(interactionSnapshot).not.toHaveBeenCalled();
});
it("ignores an old task snapshot arriving after task selection changes",async()=>{
  let old!:(v:Awaited<ReturnType<typeof interactionSnapshot>>)=>void;
  vi.mocked(interactionSnapshot).mockImplementation((id)=>id===1?new Promise((resolve)=>{old=resolve;}):Promise.resolve({enabled:true,execution_id:null,phase:"idle",items:[]}));
  await act(async()=>root.render(<QuestionSession key="one" taskId={1} linkedIds={[]}>{(_render,status)=><div>{status}</div>}</QuestionSession>));
  await act(async()=>root.render(<QuestionSession key="two" taskId={2} linkedIds={[]}>{(_render,status)=><div>{status}</div>}</QuestionSession>));
  await act(async()=>old({enabled:true,execution_id:"old",phase:"running",items:[item()]}));expect(container.textContent).not.toContain("어떤 색상?");
});
it("coalesces streamed Markdown before and after a question without duplicating completed text",()=>{
  const first:ConvoEventLike={kind:"text_update",item_id:"m",text:"# Title",complete:false};
  let items=appendConvoEvent([],first);items=appendConvoEvent(items,{kind:"interaction",interaction_id:"q"});
  const completed={...first,text:"# Title\n\n```mermaid\ngraph LR; A-->B\n```",complete:true};items=appendConvoEvent(items,completed);
  expect(items).toHaveLength(2);expect(items[0]).toMatchObject({role:"text",complete:true,text:completed.text});expect(items[1]).toEqual({role:"interaction",interactionId:"q"});
  expect(coalesceConvoEvent([first],completed)).toEqual([completed]);
});
