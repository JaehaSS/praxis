// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import type { WikiGraph, WikiPage } from "../../lib/wiki-workspace-ipc";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const mocks = vi.hoisted(() => ({ graph: vi.fn(), read: vi.fn(), save: vi.fn(), trash: vi.fn(), settings: vi.fn(), create3D: vi.fn() }));
vi.mock("./wiki-graph-3d", () => ({ createWiki3D: mocks.create3D }));
vi.mock("../../lib/wiki-workspace-ipc", () => ({ wikiGraph: mocks.graph, wikiRead: mocks.read, wikiSave: mocks.save, wikiTrash: mocks.trash }));
vi.mock("../../lib/knowledge-vault-ipc", () => ({ vaultSettingsGet: mocks.settings }));
import { WikiWorkspace } from "./WikiWorkspace";
import { clearCaptures, getCaptures } from "../../lib/designmode/store";

const page = (id: string, title: string, body: string): WikiPage => ({ id, path: id, title, body, aliases: [], tags: [], type: "page", status: "", scope: "", source_prefix: "", outgoing: [], backlinks: [], sha256: `hash-${id}` });
const a = { ...page("a.md", "문서 A", "# 문서 A\n\n|이름|값|\n|---|---|\n|캐시|redis|\n\n[문서 B](b.md)"), aliases: ["별칭"], tags: ["운영"], outgoing: ["b.md"] };
const b = { ...page("b.md", "문서 B", "# 문서 B\n\n본문"), backlinks: ["a.md"] };
const graph: WikiGraph = { schema_version: 1, nodes: [a, b], edges: [{ source: "a.md", target: "b.md", evidence: [] }], diagnostics: [], writable: true };
let node: HTMLDivElement; let root: Root;
const flush = async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); };
beforeEach(() => { Object.values(mocks).forEach(m => m.mockReset()); mocks.create3D.mockReturnValue({ update: vi.fn(), zoom: vi.fn(), reset: vi.fn(), dispose: vi.fn() }); mocks.graph.mockResolvedValue(structuredClone(graph)); mocks.settings.mockResolvedValue({ wiki_dir: "wiki", organizer_skill: "wiki-organizer", wiki_home: "위키-시작.md" }); mocks.read.mockResolvedValue({ path: a.path, content: "---\ntitle: 문서 A\n---\n# 원본", sha256: "fresh-hash" }); clearCaptures(7); node = document.createElement("div"); document.body.append(node); root = createRoot(node); });
afterEach(() => { act(() => root.unmount()); node.remove(); });
async function render(props: Partial<Parameters<typeof WikiWorkspace>[0]> = {}) { await act(async () => { root.render(<WikiWorkspace vaultId="v" host="local" {...props} />); await flush(); }); }
function button(label: string) { const found = Array.from(node.querySelectorAll<HTMLButtonElement>("button")).find(el => el.textContent === label); if (!found) throw new Error(`button missing: ${label}`); return found; }
async function click(label: string) { await act(async () => { button(label).click(); await flush(); }); }
async function select(id: string) { await act(async () => { node.querySelector<HTMLButtonElement>(`nav button[data-page='${id}']`)!.click(); await flush(); }); }
async function type(label: string, text: string) { await act(async () => { const el = node.querySelector(`[aria-label='${label}']`) as HTMLInputElement | HTMLTextAreaElement; Object.getOwnPropertyDescriptor(el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, "value")!.set!.call(el, text); el.dispatchEvent(new Event("input", { bubbles: true })); await flush(); }); }

it("searches body/aliases, filters tags, renders a table and navigates a Markdown link", async () => {
  await render();
  await type("위키 문서 검색", "별칭");
  expect(node.querySelectorAll("nav button")).toHaveLength(1);
  await select("a.md");
  expect(node.querySelector("article table")?.textContent).toContain("redis");
  await act(async () => { (node.querySelector("article a") as HTMLAnchorElement).click(); await flush(); });
  expect(node.querySelector("article h2")?.textContent).toBe("문서 B");
  await type("위키 문서 검색", "redis"); expect(node.querySelectorAll("nav button")).toHaveLength(1);
  await type("위키 문서 검색", "없는 단어"); expect(node.textContent).toContain("검색 결과가 없습니다.");
  await type("위키 문서 검색", "");
  await act(async () => { const el = node.querySelector("select[aria-label='위키 태그']") as HTMLSelectElement; el.value = "운영"; el.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(node.querySelectorAll("nav button")).toHaveLength(1);
});

it("opens the entry document by file name so choosing the vault is enough", async () => {
  const home = page("문서/기술-위키/wiki/위키-시작.md", "위키 시작", "# 위키 시작\n\n[문서 B](../../../b.md)");
  mocks.graph.mockResolvedValue({ ...structuredClone(graph), nodes: [structuredClone(a), structuredClone(b), home] });

  await render();

  expect(node.querySelector("article h2")?.textContent).toBe("위키 시작");
});

it("falls back to the most linked document when the entry document is missing", async () => {
  mocks.settings.mockResolvedValue({ wiki_dir: "wiki", organizer_skill: "wiki-organizer", wiki_home: "없는-문서.md" });

  await render();

  // b.md만 역링크를 가진다 — 링크가 모이는 곳이 사실상의 허브다.
  expect(node.querySelector("article h2")?.textContent).toBe("문서 B");
});

it("keeps the document the reader is on when the graph refreshes", async () => {
  await render(); await select("a.md");
  await act(async () => { root.render(<WikiWorkspace vaultId="v" host="local" refreshKey={1} />); await flush(); });

  expect(node.querySelector("article h2")?.textContent).toBe("문서 A");
});

it("selects graph nodes with the keyboard", async () => {
  await render(); await click("2D");
  await act(async () => { node.querySelector("[aria-label='그래프 문서: 문서 A']")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); });
  expect(node.querySelector("article table")).not.toBeNull();
});

it("shares selection and filters across 3D and 2D while protecting an edit", async () => {
  await render();
  await act(async () => { await vi.dynamicImportSettled(); });
  const view = mocks.create3D.mock.results[0].value;
  const select3D = mocks.create3D.mock.calls[0][1];
  await act(async () => { select3D("a.md"); });
  expect(node.querySelector("article h2")?.textContent).toBe("문서 A");
  await type("위키 문서 검색", "별칭");
  expect(view.update.mock.lastCall[0].map((page: WikiPage) => page.id)).toEqual(["a.md"]);
  await click("2D");
  expect(node.querySelector('[aria-label="그래프 문서: 문서 A"]')?.getAttribute("aria-pressed")).toBe("true");
  expect(view.dispose).toHaveBeenCalledOnce();
  await click("3D"); await act(async () => { await vi.dynamicImportSettled(); });
  await type("위키 문서 검색", ""); await click("원문 편집");
  await act(async () => { mocks.create3D.mock.lastCall![1]("b.md"); });
  expect(node.querySelector("textarea")).not.toBeNull();
  expect(node.textContent).toContain("편집을 저장하거나 취소한 뒤");
  expect((node.querySelector('[aria-label="3D 그래프 문서 선택"]') as HTMLSelectElement).value).toBe("a.md");
});

it("keeps the selected document readable when 3D fails", async () => {
  await render(); await select("a.md");
  await act(async () => { await vi.dynamicImportSettled(); mocks.create3D.mock.lastCall![2](); });
  expect(node.textContent).toContain("2D로 전환했습니다");
  expect(node.querySelector("article table")).not.toBeNull();
  expect(node.querySelector('[aria-label="그래프 문서: 문서 A"]')?.getAttribute("aria-pressed")).toBe("true");
  await click("원문 편집"); expect(node.querySelector("textarea")).not.toBeNull();
});

it("creates a Markdown file without an expected hash and refreshes the graph", async () => {
  const changed = vi.fn().mockResolvedValue(undefined);
  await render({ onMutation: changed }); await click("새 문서");
  await type("문서 파일 경로", "새.md"); await type("문서 원문", "# 새 문서\n본문");
  mocks.save.mockResolvedValue({ path: "새.md", content: "# 새 문서\n본문", sha256: "new" });
  mocks.graph.mockResolvedValue({ ...graph, nodes: [...graph.nodes, page("새.md", "새 문서", "# 새 문서\n본문")] });
  await click("문서 저장");
  expect(mocks.save).toHaveBeenCalledWith("v", "새.md", "# 새 문서\n본문", null, "local");
  expect(changed).toHaveBeenCalledOnce(); expect(node.querySelector("article h2")?.textContent).toBe("새 문서");
});

it("reads the current source, preserves frontmatter and keeps edits on a conflict", async () => {
  await render(); await select("a.md"); await click("원문 편집");
  expect((node.querySelector("textarea") as HTMLTextAreaElement).value).toContain("title: 문서 A");
  await type("문서 원문", "---\ntitle: 문서 A\n---\n# 내 수정");
  mocks.save.mockRejectedValue(new Error("다른 곳에서 문서가 변경됐습니다"));
  await click("문서 저장");
  expect(mocks.save).toHaveBeenCalledWith("v", "a.md", "---\ntitle: 문서 A\n---\n# 내 수정", "fresh-hash", "local");
  expect((node.querySelector("textarea") as HTMLTextAreaElement).value).toContain("내 수정");
  expect(node.querySelector("[role='alert']")?.textContent).toContain("변경됐습니다");
  await select("b.md"); expect(node.querySelector("textarea")).not.toBeNull();
  await click("편집 취소"); await click("계속 편집"); expect(node.querySelector("textarea")).not.toBeNull();
  await click("편집 취소"); await click("수정 버리기"); expect(node.querySelector("textarea")).toBeNull();
});

it("distinguishes successful save from a failed refresh and disables stale mutation", async () => {
  await render(); await select("a.md"); await click("원문 편집");
  mocks.save.mockResolvedValue({ path: "a.md", content: "# 원본", sha256: "saved" });
  mocks.graph.mockRejectedValue(new Error("parser unavailable"));
  await click("문서 저장");
  expect(node.textContent).toContain("문서를 저장했습니다."); expect(node.textContent).toContain("갱신에 실패했습니다");
  expect(node.querySelector("textarea")).toBeNull(); expect(button("원문 편집").disabled).toBe(true);
});

it("requires delete confirmation, preserves the document on failure and refreshes after trash", async () => {
  await render(); await select("b.md"); await click("삭제");
  expect(node.textContent).toContain("문서 1개의 링크가 끊어집니다");
  await click("삭제 취소"); expect(mocks.trash).not.toHaveBeenCalled();
  await click("삭제"); mocks.trash.mockRejectedValueOnce(new Error("휴지통 이동 실패")); await click("휴지통으로 이동");
  expect(node.querySelector("article h2")?.textContent).toBe("문서 B");
  mocks.trash.mockResolvedValue(undefined); mocks.graph.mockResolvedValue({ ...graph, nodes: [{ ...a, outgoing: [] }], edges: [], diagnostics: [{ kind: "broken_link", source: "a.md", target: "b.md" }] });
  await click("휴지통으로 이동");
  expect(mocks.trash).toHaveBeenLastCalledWith("v", "b.md", "hash-b.md", "local");
  expect(node.querySelectorAll("nav button")).toHaveLength(1); expect(node.textContent).toContain("연결 점검 (1)");
});

it("ignores a previous vault's late graph response", async () => {
  let finish!: (value: WikiGraph) => void;
  mocks.graph.mockImplementationOnce(() => new Promise<WikiGraph>(resolve => { finish = resolve; }));
  await render();
  mocks.graph.mockResolvedValue({ ...graph, nodes: [page("new.md", "다른 창고", "# 새 폴더")] });
  await render({ vaultId: "other" });
  await act(async () => { finish(graph); await flush(); });
  expect(node.textContent).toContain("다른 창고"); expect(node.querySelectorAll("nav button")).toHaveLength(1);
});

it("disables file mutations for a read-only vault", async () => {
  mocks.graph.mockResolvedValue({ ...graph, writable: false });
  await render(); await select("a.md");
  expect(button("새 문서").disabled).toBe(true); expect(button("원문 편집").disabled).toBe(true); expect(button("삭제").disabled).toBe(true);
});

it("does not open an old vault's edit buffer when its read resolves late", async () => {
  let finish!: (value: { path: string; content: string; sha256: string }) => void;
  await render(); await select("a.md");
  mocks.read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  await click("원문 편집");
  mocks.graph.mockResolvedValue({ ...graph, nodes: [page("new.md", "새 창고 문서", "# 새 창고")] });
  await render({ vaultId: "other" });
  await act(async () => { finish({ path: "a.md", content: "old private buffer", sha256: "old" }); await flush(); });
  expect(node.querySelector("textarea")).toBeNull();
  expect(node.textContent).toContain("새 창고 문서"); expect(node.textContent).not.toContain("old private buffer");
});

const PREFIX = "---\ntitle: 문서 A\n---\n";
const withEvidence = (): WikiGraph => ({
  ...structuredClone(graph),
  nodes: [{ ...structuredClone(a), source_prefix: PREFIX }, structuredClone(b)],
  // 하네스가 세는 줄 번호는 프런트매터를 포함한 원문 기준이다. 본문만 세면 링크가 4줄 위로 어긋난다.
  edges: [{ source: "a.md", target: "b.md", evidence: [{ line: 10, target: "b.md", syntax: "markdown", anchor: "" }] }],
});
const openSource = () => Array.from(node.querySelectorAll("details")).find(el => el.textContent?.includes("원문 보기"))!;

it("shows the link evidence and opens the numbered source at that very line", async () => {
  mocks.graph.mockResolvedValue(withEvidence());
  await render();
  await select("a.md");

  expect(node.textContent).toContain("연결 근거 (1)");
  expect(openSource().textContent).toContain("원문 보기 (10줄)");

  await click("10번째 줄");

  expect(openSource().open).toBe(true);
  const active = node.querySelector('li[aria-current="true"]')!;
  expect(active.textContent).toBe("10[문서 B](b.md)");
});

it("follows a backlink evidence into the citing document and its line", async () => {
  mocks.graph.mockResolvedValue(withEvidence());
  await render();
  await select("b.md");

  expect(node.textContent).toContain("이 문서를 가리킨 자리 (1)");

  await click("문서 A 10번째 줄");

  expect(node.querySelector("article h2")?.textContent).toBe("문서 A");
  expect(openSource().open).toBe(true);
  expect(node.querySelector('li[aria-current="true"]')?.textContent).toBe("10[문서 B](b.md)");
});

it("filters by document type and by a tag chip in the reader", async () => {
  const note = { ...page("c.md", "회의록", "# 회의록"), type: "meeting", tags: ["운영"] };
  mocks.graph.mockResolvedValue({ ...structuredClone(graph), nodes: [structuredClone(a), structuredClone(b), note] });
  await render();

  await act(async () => {
    const el = node.querySelector("select[aria-label='위키 문서 종류']") as HTMLSelectElement;
    el.value = "meeting"; el.dispatchEvent(new Event("change", { bubbles: true })); await flush();
  });
  expect(Array.from(node.querySelectorAll("nav button")).map(el => el.textContent)).toEqual(["회의록c.md"]);

  await act(async () => {
    const el = node.querySelector("select[aria-label='위키 문서 종류']") as HTMLSelectElement;
    el.value = ""; el.dispatchEvent(new Event("change", { bubbles: true })); await flush();
  });
  await select("a.md");
  await act(async () => { (node.querySelector("[aria-label='태그로 거르기: 운영']") as HTMLButtonElement).click(); await flush(); });

  expect(Array.from(node.querySelectorAll("nav button")).map(el => el.textContent)).toEqual(["문서 Aa.md", "회의록c.md"]);
});

it("floats a title hit above a body hit and shows where the body matched", async () => {
  // 두 문서 모두 "문서"를 갖지만 b.md는 제목에, a.md는 본문(표)에 있다.
  mocks.graph.mockResolvedValue({ ...structuredClone(graph), nodes: [structuredClone(a), { ...structuredClone(b), title: "캐시 문서" }] });
  await render();
  await type("위키 문서 검색", "캐시");

  const listed = Array.from(node.querySelectorAll<HTMLButtonElement>("nav button"));
  expect(listed.map(el => el.dataset.page)).toEqual(["b.md", "a.md"]);
  // 제목이 맞은 쪽은 위에 이미 보이므로 조각을 되풀이하지 않는다.
  expect(listed[0].querySelector("mark")).toBeNull();
  expect(listed[1].querySelector("mark")?.textContent).toBe("캐시");
  expect(listed[1].textContent).toContain("|캐시|redis|");
});

it("attaches the open document to the conversation through the capture chips", async () => {
  await render({ taskId: 7, vaultRoot: "/창고/" });
  await select("a.md");
  await click("대화에 첨부");

  const [capture] = getCaptures(7);
  expect(capture.source).toBe("wiki");
  expect(capture.file_path).toBe("/창고/a.md");
  expect(capture.outer_html).toBe("문서 A");
  // 원문 보기와 같은 것을 보내야 한다 — 프런트매터까지.
  expect(capture.selection_text).toBe(`${a.source_prefix}${a.body}`);
  expect(node.textContent).toContain("대화에 첨부했습니다");
});

it("cannot attach without a conversation to attach to", async () => {
  await render({ vaultRoot: "/창고" });
  await select("a.md");

  expect(button("대화에 첨부").disabled).toBe(true);
  expect(button("대화에 첨부").title).toBe("대화를 하나 연 뒤에 첨부할 수 있습니다.");
});
