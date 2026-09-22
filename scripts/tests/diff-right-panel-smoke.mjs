import assert from "node:assert/strict";
import { mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright";
import { createServer } from "vite";
import react from "@vitejs/plugin-react";

// Real workspace components, with only the native diff/skills transport mocked.
const directory = path.resolve(".praxis/verification/diff-right-panel-smoke");
const fixture = path.join(directory, "ui-fixture");
await mkdir(fixture, { recursive: true });
await writeFile(
  path.join(fixture, "index.html"),
  '<div id="root"></div><script type="module" src="./main.tsx"></script>',
);
await writeFile(
  path.join(fixture, "main.tsx"),
  String.raw`
import React, { useCallback, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { registerTransport } from "/src/lib/transport";
import { DiffSessionProvider } from "/src/components/DiffSessionContext";
import { useSessionPanels } from "/src/components/ide/useSessionPanels";
import { WorkspaceSplit } from "/src/components/ide/WorkspaceSplit";
import { CodeColumnTabs } from "/src/components/ide/CodeColumnTabs";
import { ChangesList } from "/src/components/ide/ChangesList";
import { SessionDiffSurface } from "/src/components/ide/SessionDiffSurface";
import { ConversationView } from "/src/components/ide/ConversationView";
import { AgentComposer } from "/src/components/ide/AgentComposer";
import { requestComposerFocus } from "/src/lib/composer-focus";
import "/src/index.css";

const files = [
  { path: "src/alpha.ts", status: "M", patch: "@@ -1 +1 @@\\n-export const alpha = 1;\\n+export const alpha = 2;" },
  { path: "src/beta.ts", status: "A", patch: "@@ -0,0 +1 @@\\n+export const beta = true;" },
];

// This is the complete boundary exercised by the composed components. No Tauri process or
// repository is contacted; unsupported review actions are intentionally not invoked here.
registerTransport({
  hostId: "smoke",
  kind: "remote",
  taskDiff: async () => ({ files, baseline: { kind: "pinned" } }),
  diffHunks: async () => [],
  annotationsList: async () => [],
  skillsList: async () => [],
  knowledgeSearch: async () => [],
} as any);

const baseItems = Array.from({ length: 90 }, (_, index) => ({
  role: "text" as const,
  text: "대화 행 " + index + " — 숨김 중에도 유지되어야 하는 충분히 긴 대화 내용입니다.",
}));

function SessionContents({ stream, hidden }: { stream: number; hidden: boolean }) {
  const items = useMemo(
    () => [
      ...baseItems,
      ...Array.from({ length: stream }, (_, index) => ({ role: "text" as const, text: "스트리밍 업데이트 " + index })),
    ],
    [stream],
  );
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-chat-retained>
      <ConversationView conversationId={42} items={items} busy hidden={hidden} />
    </div>
  );
}

function Harness() {
  const panels = useSessionPanels("smoke:42");
  const [stream, setStream] = useState(0);
  const [draft, setDraft] = useState("");
  const openDiff = useCallback((path: string) => {
    panels.setCentralDiffPath(path);
    panels.openDiffPanel();
  }, [panels]);
  const close = useCallback(() => panels.setCodeOpen(false), [panels]);
  const returnToConversation = useCallback(() => {
    panels.setCentralDiffPath(null);
    requestAnimationFrame(() => requestComposerFocus(42));
  }, [panels]);
  const activate = useCallback((tab: "activity" | "file" | "preview" | "diff") => {
    panels.setCodeTab(tab);
    panels.setCodeOpen(true);
  }, [panels]);
  const code = (
    <div className="min-h-0 flex-1" data-diff-right-panel>
      <CodeColumnTabs
        active={panels.codeTab}
        onActivate={activate}
        previewAvailable
        editorPoppedOut={false}
        onClose={close}
        activity={<div>작업정보</div>}
        file={<div>파일</div>}
        preview={<div>프리뷰</div>}
        diff={<ChangesList />}
      />
    </div>
  );
  return (
    <DiffSessionProvider task={{ host: "smoke", id: 42 }} openDiff={openDiff}>
      <main className="flex h-screen min-w-0 flex-col overflow-hidden bg-bg text-text" data-smoke-shell>
        <header className="flex shrink-0 items-center gap-2 border-b border-border p-2">
          <button aria-label="Diff 보기" onClick={panels.toggleDiffPanel}>Diff</button>
          <button aria-label="스트리밍 업데이트" onClick={() => setStream((value) => value + 1)}>stream</button>
        </header>
        <WorkspaceSplit
          session={
            <SessionDiffSurface
              path={panels.centralDiffPath}
              onBack={returnToConversation}
              onOpenPath={openDiff}
            >
              <SessionContents stream={stream} hidden={panels.centralDiffPath != null} />
            </SessionDiffSurface>
          }
          code={code}
          codeOpen={panels.codeOpen}
          diffOpen={panels.codeOpen && panels.codeTab === "diff"}
          onCloseCode={close}
        />
        <footer hidden={panels.centralDiffPath != null} inert={panels.centralDiffPath != null} data-composer>
          <AgentComposer
            value={draft}
            onChange={setDraft}
            onSend={() => {}}
            onInterrupt={() => {}}
            onHistory={() => {}}
            files={files.map((file) => file.path)}
            repo="/mock/repo"
            taskId={42}
            host="smoke"
            active={panels.centralDiffPath == null}
          />
        </footer>
      </main>
    </DiffSessionProvider>
  );
}

createRoot(document.getElementById("root")!).render(<Harness />);
`,
);

let server;
let browser;
let page;
const outcomes = [];
try {
  server = await createServer({
    configFile: false,
    plugins: [react()],
    server: { host: "127.0.0.1", port: 0 },
    logLevel: "error",
  });
  await server.listen();
  browser = await chromium.launch({ headless: true, channel: process.env.PRAXIS_SMOKE_BROWSER || "chrome" });
  page = await browser.newPage({ viewport: { width: 1280, height: 760 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const url = server.resolvedUrls.local[0] + path.relative(process.cwd(), fixture) + "/index.html";
  const panel = page.locator("[data-diff-right-panel]");
  const surface = page.locator("[data-session-diff-surface]");
  const central = page.locator("[data-central-diff]");
  const chat = page.locator("[data-chat-retained]");
  const composer = page.locator("[data-composer] textarea");
  const alpha = page.getByRole("button", { name: /alpha\.ts/ });
  const beta = page.getByRole("button", { name: /beta\.ts/ });

  await page.goto(url);
  await composer.fill("보존할 초안");
  await page.getByRole("button", { name: "Diff 보기" }).click();
  await alpha.waitFor();
  assert.equal(await page.getByRole("tab", { name: "Diff" }).getAttribute("aria-selected"), "true");
  assert.equal(await central.count(), 0);
  outcomes.push("Diff opens the right ChangesList while conversation and composer remain rendered");

  const chatScroll = chat.locator(".overflow-auto").first();
  const beforeStream = await chatScroll.evaluate((node) => {
    node.scrollTop = 120;
    node.dispatchEvent(new Event("scroll", { bubbles: true }));
    return node.scrollTop;
  });
  await alpha.click();
  await central.waitFor();
  assert.equal(await panel.isVisible(), true);
  assert.match(await central.textContent(), /src\/alpha\.ts/);
  assert.equal(await alpha.isVisible(), true);
  await page.getByRole("button", { name: "스트리밍 업데이트" }).click();
  await page.getByRole("button", { name: "스트리밍 업데이트" }).click();
  await page.waitForTimeout(30);
  assert.equal(await chat.count(), 1);
  assert.equal(await chatScroll.evaluate((node) => node.scrollTop), beforeStream);
  assert.equal(await composer.inputValue(), "보존할 초안");
  const hiddenFocusAccepted = await composer.evaluate((node) => {
    node.focus();
    return document.activeElement === node;
  });
  assert.equal(hiddenFocusAccepted, false);
  outcomes.push("A keeps the same right list; hidden streaming preserves chat scroll, draft, and blocks focus");

  await page.keyboard.press("]");
  await page.waitForFunction(() => document.querySelector("[data-central-diff]")?.textContent?.includes("src/beta.ts"));
  assert.equal(await beta.isVisible(), true);
  outcomes.push("Diff keyboard navigation advances A to B without closing the list");

  await page.getByRole("button", { name: "대화로 돌아가기" }).click();
  await central.waitFor({ state: "detached" });
  assert.equal(await composer.inputValue(), "보존할 초안");
  assert.equal(await chatScroll.evaluate((node) => node.scrollTop), beforeStream);
  await page.waitForFunction(() => document.activeElement === document.querySelector("[data-composer] textarea"));
  assert.equal(await composer.evaluate((node) => document.activeElement === node), true);
  outcomes.push("Back restores the retained conversation scroll, draft, and composer focus");

  // Q-1: a follow-mode transcript resumes only after the first reveal, so output which arrived
  // while hidden cannot displace the reader before the saved scroll position is restored.
  const tallDraft = Array.from({ length: 12 }, (_, index) => "긴 초안 " + index).join("\n");
  await composer.fill(tallDraft);
  const followedBefore = await chatScroll.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
    node.dispatchEvent(new Event("scroll", { bubbles: true }));
    return node.scrollTop;
  });
  await alpha.click();
  await central.waitFor();
  for (let index = 0; index < 3; index += 1) {
    await page.getByRole("button", { name: "스트리밍 업데이트" }).click();
  }
  await page.getByRole("button", { name: "대화로 돌아가기" }).click();
  await central.waitFor({ state: "detached" });
  assert.equal(await chatScroll.evaluate((node) => node.scrollTop), followedBefore);
  await page.getByRole("button", { name: "스트리밍 업데이트" }).click();
  await page.waitForTimeout(30);
  assert(await chatScroll.evaluate((node) => node.scrollTop >= node.scrollHeight - node.clientHeight));
  outcomes.push("Q-1 preserves bottom scroll across a tall composer and hidden multi-stream, then follows later output");

  await composer.fill("보존할 초안");

  await alpha.click();
  await central.waitFor();
  await page.getByRole("tab", { name: "프리뷰" }).click();
  await central.waitFor({ state: "detached" });
  assert.equal(await composer.inputValue(), "보존할 초안");
  await page.getByRole("tab", { name: "Diff" }).click();
  await alpha.waitFor();
  await page.getByRole("button", { name: "코드 열 닫기" }).click();
  assert.equal(await panel.count(), 0);
  assert.equal(await composer.inputValue(), "보존할 초안");
  outcomes.push("Preview and close both return to conversation without losing the draft");

  await page.getByRole("button", { name: "Diff 보기" }).click();
  await alpha.waitFor();
  for (const width of [960, 1280, 1440]) {
    await page.setViewportSize({ width, height: 760 });
    await page.waitForTimeout(30);
    const [centerBox, listBox, shell] = await Promise.all([
      surface.boundingBox(),
      panel.boundingBox(),
      page.locator("[data-smoke-shell]").evaluate((node) => ({ scrollWidth: node.scrollWidth, clientWidth: node.clientWidth })),
    ]);
    assert(centerBox && listBox, "missing layout boxes at " + width + "px");
    assert(listBox.x >= centerBox.x + centerBox.width, "list is not right of center at " + width + "px");
    assert(listBox.width <= 320.5, "list exceeds 320px at " + width + "px");
    assert(shell.scrollWidth <= shell.clientWidth, "workspace overflows at " + width + "px");
    outcomes.push(width + "px keeps the list right of center without horizontal overflow");
  }
  await page.locator("[data-smoke-shell]").evaluate((node) => {
    node.style.width = "512px";
  });
  await page.waitForTimeout(30);
  const narrowShell = await page.locator("[data-smoke-shell]").evaluate((node) => ({
    scrollWidth: node.scrollWidth,
    clientWidth: node.clientWidth,
  }));
  assert(
    narrowShell.scrollWidth <= narrowShell.clientWidth,
    "512px center overflows: " + narrowShell.scrollWidth + "/" + narrowShell.clientWidth,
  );
  const [panelBox, tabListBox, diffTabBox, closeBox, diffTabWhiteSpace] = await Promise.all([
    panel.boundingBox(),
    panel.getByRole("tablist").boundingBox(),
    panel.getByRole("tab", { name: "Diff" }).boundingBox(),
    panel.getByRole("button", { name: "코드 열 닫기" }).boundingBox(),
    panel.getByRole("tab", { name: "Diff" }).evaluate((node) => getComputedStyle(node).whiteSpace),
  ]);
  assert(panelBox && tabListBox && diffTabBox && closeBox, "missing narrow tab or close control");
  assert.equal(diffTabWhiteSpace, "nowrap");
  assert(diffTabBox.x >= tabListBox.x && diffTabBox.x + diffTabBox.width <= tabListBox.x + tabListBox.width, "active Diff tab is clipped");
  assert(closeBox.x >= tabListBox.x + tabListBox.width && closeBox.x + closeBox.width <= panelBox.x + panelBox.width, "close control is clipped");
  await panel.getByRole("button", { name: "코드 열 닫기" }).click({ trial: true });
  outcomes.push("512px available center keeps the Diff list, single-line active tab, and fixed close control visible");

  await page.locator("[data-smoke-shell]").evaluate((node) => {
    node.style.width = "";
  });
  await page.setViewportSize({ width: 1280, height: 760 });
  await page.waitForTimeout(30);
  await alpha.click();
  await central.waitFor();
  await page.screenshot({ path: path.join(directory, "diff-right-panel-central.png") });
  outcomes.push("central Diff screenshot captured at 1280px");
  assert.deepEqual(errors, []);
  console.log("PASS diff right-panel smoke (" + outcomes.length + " checks):\n- " + outcomes.join("\n- ") + "\nScreenshot: " + path.join(directory, "diff-right-panel-central.png"));
} catch (error) {
  if (page) await page.screenshot({ path: path.join(directory, "diff-right-panel-failure.png") }).catch(() => {});
  throw error;
} finally {
  await browser?.close();
  await server?.close();
  await rm(fixture, { recursive: true, force: true });
}
