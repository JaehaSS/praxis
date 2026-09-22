import assert from "node:assert/strict";
import { mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";

import react from "@vitejs/plugin-react";
import { chromium } from "playwright";
import { createServer } from "vite";

const fixture = path.resolve(".praxis/verification/knowledge-vault-smoke/ui-fixture");
const screenshots = [];
let browser;
let server;

await mkdir(fixture, { recursive: true });
await writeFile(path.join(fixture, "index.html"), '<div id="root"></div><script type="module" src="./main.tsx"></script>');
await writeFile(path.join(fixture, "main.tsx"), `
import React from "react";
import { createRoot } from "react-dom/client";
import { VaultView } from "/src/components/knowledge-vault/VaultView";
import { VaultReferences } from "/src/components/knowledge-vault/VaultReferences";
import "/src/index.css";
document.documentElement.classList.add("dark");
createRoot(document.getElementById("root")).render(<main className="min-h-screen bg-bg p-4 text-text"><VaultView host="local" repo="/workspace/alpha" /><VaultReferences host="local" repo="/workspace/alpha" query="문서 정리" clientRef="create-ref" taskId={9} /><VaultReferences host="remote" repo="/workspace/alpha" query="원격" clientRef="remote-ref" taskId={10} /></main>);
`);
await writeFile(path.join(fixture, "dialog.ts"), `
export const open = async () => window.__vaultDialogResponses.shift() ?? null;
`);
await writeFile(path.join(fixture, "api.ts"), `
let connected = false;
let proposalPending = true;
let jobState = "failed";
let docs = [doc("source", "원본 자료", "source"), doc("note", "기존 노트", "note")];
const calls = () => window.__vaultCalls;
const revision = id => ({ id: id + "-revision", document_id: id, relative_path: id + ".md", sha256: "a".repeat(64), size: 42, predecessor: null });
function doc(id, title, kind) { return { id, vault_id: "vault", kind, title, state: "active", current_revision_id: id + "-revision", current_scope: id === "source" ? "common" : "private-data" }; }
function add(id, title, kind) { const next = doc(id, title, kind); docs = [...docs.filter(item => item.id !== id), next]; return { document_id: id, revision_id: next.current_revision_id, sha256: "a".repeat(64) }; }
export const vaultStatus = async () => ({ supported: true, vaults: connected ? [{ id: "vault", vault_root: "/vault", enabled: true }] : [], auto_enabled: false, provider_available: false, current_provider: "", current_provider_display: "", project_binding: null, current_consent: null, prior_bindings: [], operation_conflicts: [], index_rebuild_needed: false });
export const vaultConnect = async path => { calls().push(["connect", path]); connected = true; return { id: "vault", vault_root: path, enabled: true }; };
export const vaultDisconnect = async () => { connected = false; };
export const vaultDocuments = async () => docs;
export const vaultDocument = async id => { const document = docs.find(item => item.id === id); const current_revision = revision(id); return { document, current_revision, revision_history: [current_revision], bounded_text: "사람이 읽을 수 있는 자료 내용입니다.", read_error: null, unsupported_reason: null, source_revisions: id === "note" || id === "manual-note" ? [{ document_id: "source", revision_id: "source-revision", title: "원본 자료" }] : [], backlinks: id === "source" ? [{ document_id: "note", revision_id: "note-revision", title: "기존 노트" }] : [], index_status: "indexed", index_reason: null }; };
export const vaultArchive = async () => {};
export const vaultScope = async () => {};
export const vaultCreateNote = async (_vault, title) => add("manual-note", title, "note");
export const vaultUpdateNote = async (_vault, documentId) => ({ document_id: documentId, revision_id: documentId + "-updated" });
export const vaultSearch = async (query, offset = 0) => { const hits = docs.filter(item => item.title.includes(query) || query.includes("문서")).map(item => ({ document_id: item.id })); return { hits: hits.slice(offset, offset + 100), has_more: hits.length > offset + 100 }; };
export const vaultScan = async () => ({ indexed: 0, skipped: 0, partial: false, warnings: [] });
export const vaultImportFiles = async (_vault, paths) => paths.map(file => ({ path: file, source: add("file", file.split("/").at(-1), "source"), error: null }));
export const vaultCreateTextSource = async (_vault, title) => add("text", title, "source");
export const vaultCreateUrlSource = async (_vault, title) => add("url", title, "url");
export const vaultInspectDrift = async id => ({ document_id: id, revision_id: id + "-revision", expected_hash: "a", observed_hash: "a" });
export const vaultAcceptDrift = async () => {};
export const vaultOpenOriginal = async () => "/vault/source.md";
export const vaultProposals = async () => [{ id: "proposal", vault_id: "vault", title: "정리 제안", status: proposalPending ? "pending" : "accepted", requested_scope: "private-data" }];
export const vaultProposal = async () => ({ proposal: { id: "proposal", vault_id: "vault", title: "정리 제안", status: proposalPending ? "pending" : "accepted", requested_scope: "private-data" }, candidate_body: "제안 초안", why: "자료를 정리할 수 있습니다.", base_revision: null, target_document_id: null, target_title: null, target_base_hash: null, target_body: null, source_revisions: ["source-revision"], completion_snapshots: [], review_token: null });
export const vaultReviewProposal = async () => ({ proposal_id: "proposal", value: "review-token" });
export const vaultApproveProposal = async () => { proposalPending = false; return add("proposal-note", "정리 제안", "note"); };
export const vaultDismissProposal = async () => { proposalPending = false; };
export const vaultJobs = async () => [{ id: "job", attempt_id: "attempt", vault_id: "vault", terminal_snapshot_id: "snapshot", provider: "local", state: jobState, failure_reason: jobState === "failed" ? "일시적 오류" : null }];
export const vaultRetryJob = async () => { jobState = "completed"; };
export const vaultPreview = async (_repo, query, clientRef, host) => { calls().push(["preview", query, clientRef, host]); return { id: "preview", query_hash: "hash", created_at: 1788735600, references: [{ document_id: "source", title: "원본 자료", scope: "common", revision_id: "source-revision", revision_hash: "a".repeat(64), snippet: "관련 문서 발췌입니다.", reason: "요청과 일치", excluded: false, stale_reason: null }] }; };
export const vaultExcludePreviewReference = async (preview, revisionId, host) => { calls().push(["exclude", preview, revisionId, host]); };
export const vaultUsage = async (taskId, host) => { calls().push(["usage", taskId, host]); return [{ attempt_id: "attempt", revision_id: "source-revision", revision_hash: "a".repeat(64), snippet: "전달된 발췌", snippet_hash: "b".repeat(64), delivery_state: "delivered", citation_state: "unknown" }]; };
export const vaultSetDraftPolicy = async (...args) => { calls().push(["policy", ...args]); };
export const vaultRegisterProject = async () => ({ id: "binding", epoch: "1", canonical_repo_root: "/workspace/alpha" });
export const vaultRebindProject = async () => ({ id: "binding", epoch: "2", canonical_repo_root: "/workspace/alpha" });
export const vaultSetAutoSuggestions = async () => {};
export const vaultSetCaptureConsent = async () => ({ id: "consent", provider: "local" });
export const vaultRevokeCaptureConsent = async () => {};
export const vaultRebind = async () => {};
export const vaultRecoverOperations = async () => ({ recovered: 0, conflicts: 0, reindex_needed: 0 });
export const vaultPrepareManualAnalysis = async () => ({ review_id: "manual-review", token: "manual-token", provider: "fixture", provider_display: "실행하지 않는 테스트 모델", provider_available: false, resulting_scope: "private-data", sources: [] });
export const vaultExecuteManualAnalysis = async () => { throw new Error("The browser fixture never executes an analysis provider"); };
`);

try {
  server = await createServer({
    configFile: false,
    plugins: [{
      name: "knowledge-vault-fixture",
      enforce: "pre",
      resolveId(source) {
        if (source.endsWith("/knowledge-vault-ipc")) return path.join(fixture, "api.ts");
        if (source === "@tauri-apps/plugin-dialog") return path.join(fixture, "dialog.ts");
      },
    }, react()],
    server: { host: "127.0.0.1", port: 0 },
    logLevel: "error",
  });
  await server.listen();
  browser = await chromium.launch({ headless: true, channel: process.env.PRAXIS_SMOKE_BROWSER || "chrome" });
  const page = await browser.newPage({ viewport: { width: 1360, height: 940 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(() => { window.__vaultDialogResponses = [null, "/vault", ["/imports/file.md"]]; window.__vaultCalls = []; });
  await page.goto(server.resolvedUrls.local[0] + path.relative(process.cwd(), fixture) + "/index.html");
  const shot = async (name) => { const file = `/tmp/praxis-knowledge-vault-${name}.png`; await page.screenshot({ path: file }); screenshots.push(file); };

  await page.getByRole("button", { name: "디렉터리 선택", exact: true }).click();
  assert.equal(await page.evaluate(() => window.__vaultCalls.filter(([name]) => name === "connect").length), 0);
  await page.getByRole("button", { name: "디렉터리 선택", exact: true }).click();
  await page.getByRole("button", { name: "연결", exact: true }).click();
  await page.getByRole("button", { name: /원본 자료/ }).waitFor();
  await shot("dark");
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await shot("light");
  await page.setViewportSize({ width: 390, height: 844 });
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  await shot("narrow");
  await page.setViewportSize({ width: 1360, height: 940 });
  await page.evaluate(() => document.documentElement.classList.add("dark"));

  await page.getByRole("button", { name: "파일 선택", exact: true }).click();
  await page.getByRole("button", { name: "파일 고르기", exact: true }).click();
  await page.getByRole("button", { name: "추가", exact: true }).click();
  await page.getByText("file.md", { exact: true }).waitFor();
  await page.getByRole("button", { name: "텍스트 자료", exact: true }).click();
  await page.getByLabel("제목", { exact: true }).fill("붙여 넣은 자료");
  await page.getByLabel("내용", { exact: true }).fill("텍스트 내용");
  await page.getByRole("button", { name: "추가", exact: true }).click();
  await page.getByText("붙여 넣은 자료", { exact: true }).waitFor();
  await page.getByRole("button", { name: "URL + 메모", exact: true }).click();
  await page.getByLabel("제목", { exact: true }).fill("URL 자료");
  await page.getByLabel("URL", { exact: true }).fill("https://example.test");
  await page.getByLabel("내 메모", { exact: true }).fill("읽은 이유");
  await page.getByRole("button", { name: "추가", exact: true }).click();
  await page.getByText("URL 자료", { exact: true }).waitFor();

  await page.getByRole("button", { name: /원본 자료/ }).click();
  await page.getByRole("button", { name: "기존 노트", exact: true }).click();
  await page.getByRole("button", { name: "원본 자료", exact: true }).click();
  await page.getByRole("button", { name: "노트 만들기", exact: true }).click();
  await page.getByLabel("노트 본문", { exact: true }).fill("손으로 쓴 정리");
  await page.getByRole("button", { name: "노트 저장", exact: true }).click();
  await page.getByRole("button", { name: "정리된 지식", exact: true }).click();
  await page.getByText("원본 자료 노트", { exact: true }).waitFor();

  await page.getByRole("button", { name: "제안 검토", exact: true }).click();
  await page.getByRole("button", { name: /정리 제안/ }).waitFor();
  await page.getByText("분석 작업 1건", { exact: true }).click();
  await page.getByRole("button", { name: "이 분석 다시 시도", exact: true }).click();
  await page.getByLabel("제안 본문", { exact: true }).fill("검토한 제안");
  await page.getByRole("button", { name: "검토 갱신", exact: true }).click();
  await page.getByRole("button", { name: "저장", exact: true }).click();
  await page.getByRole("button", { name: /정리 제안.*저장됨/ }).waitFor();

  await page.getByRole("button", { name: "관련 자료 미리보기", exact: true }).click();
  await page.getByText("관련 문서 발췌입니다.", { exact: true }).waitFor();
  await page.getByRole("button", { name: "제외", exact: true }).click();
  await page.getByText("제외됨", { exact: true }).waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("region", { name: "작업 참고 자료", exact: true }).scrollIntoViewIfNeeded();
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), "reference metadata must fit the narrow viewport");
  await shot("references-narrow");
  const calls = await page.evaluate(() => window.__vaultCalls);
  assert.equal(calls.filter(([name]) => name === "preview").length, 1);
  assert.equal(calls.some(([, , , host]) => host === "remote"), false);
  assert.deepEqual(errors, []);
  console.log("PASS: actual VaultView and VaultReferences exercised with synthetic IPC only; local connect cancellation, file/text/URL imports, detail links/note, proposal review/approve/jobs, preview exclusion, usage, and remote guard.");
  console.log(screenshots.join("\n"));
} finally {
  await browser?.close();
  await server?.close();
  await rm(fixture, { recursive: true, force: true });
}
