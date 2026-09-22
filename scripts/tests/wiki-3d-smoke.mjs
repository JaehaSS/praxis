import assert from "node:assert/strict";
import { mkdir, writeFile, rm } from "node:fs/promises";
import path from "node:path";
import { chromium, webkit } from "playwright";
import { createServer } from "vite";
import react from "@vitejs/plugin-react";

const useWebKit = process.env.PRAXIS_SMOKE_BROWSER === "webkit";
const directory = path.resolve(".praxis/verification/wiki-3d", useWebKit ? "webkit" : "chrome");
const fixture = path.join(directory, "fixture");
const source = file => `/@fs/${path.resolve(file)}`;
let browser, server;
try {
  await mkdir(fixture, { recursive: true });
  await writeFile(path.join(fixture, "index.html"), '<!doctype html><meta charset="utf-8"><link rel="icon" href="data:,"><div id="root"></div><script type="module" src="./main.tsx"></script>');
  await writeFile(path.join(fixture, "data.ts"), `
const count = Number(new URLSearchParams(location.search).get('count') || 24);
const nodes = Array.from({length:count}, (_, i) => ({id:'wiki/'+i+'.md',path:'wiki/'+i+'.md',title:i===2?'<img src=x onerror="window.XSS=1">':'문서 '+i,body:'# 문서 '+i+'\\n\\n안전한 Markdown 본문',aliases:[],tags:[i%2?'개발':'기획'],source_prefix:'wiki',outgoing:i>0?['wiki/'+Math.floor((i-1)/3)+'.md']:[],backlinks:[],sha256:'hash-'+i}));
const edges = nodes.slice(1).map(node => ({source:node.id,target:node.outgoing[0],evidence:[]}));
for(const edge of edges) nodes.find(node=>node.id===edge.target).backlinks.push(edge.source);
export const vaultSettingsGet = async () => ({wiki_home:'0.md'});
export const wikiGraph = async () => structuredClone({schema_version:1,nodes,edges,diagnostics:[],writable:true});
export const wikiRead = async (_vault,path) => { const node=nodes.find(node=>node.path===path); return {path,content:node.body,sha256:node.sha256}; };
export const wikiSave = async (_vault,path,content) => { const node=nodes.find(node=>node.path===path); node.title=content.split('\\n')[0].replace(/^# /,''); node.body=content; node.sha256='changed'; return {path,content,sha256:node.sha256}; };
export const wikiTrash = async () => { throw new Error('Not part of this fixture'); };
`);
  await writeFile(path.join(fixture, "graph.ts"), `
import ForceGraph3D from '${source("node_modules/3d-force-graph/dist/3d-force-graph.mjs")}';
window.__graphs=[]; window.__disposed=0;
export default class InstrumentedGraph {
  constructor(element, options) {
    const graph = new ForceGraph3D(element, options);
    const dispose = graph._destructor;
    graph._destructor = (...args) => { window.__disposed++; return dispose(...args); };
    const onHover = graph.onNodeHover;
    graph.onNodeHover = callback => onHover((node, previous) => { window.__hover=node?.id; callback(node, previous); });
    window.__graphs.push(graph); return graph;
  }
}
`);
  await writeFile(path.join(fixture, "main.tsx"), `
import React from 'react';
import { createRoot } from 'react-dom/client';
import '${source("src/index.css")}';
import { WikiWorkspace } from '${source("src/components/knowledge-vault/WikiWorkspace.tsx")}';
createRoot(document.getElementById('root')).render(<main style={{height:'100vh',overflow:'auto',padding:12,containerType:'inline-size'}}><WikiWorkspace vaultId="fixture" host="local"/></main>);
`);
  server = await createServer({ configFile: false, root: fixture, plugins: [react()], resolve: { alias: [
    { find: /^.*\/wiki-workspace-ipc$/, replacement: path.join(fixture, "data.ts") },
    { find: /^.*\/knowledge-vault-ipc$/, replacement: path.join(fixture, "data.ts") },
    { find: "3d-force-graph", replacement: path.join(fixture, "graph.ts") },
  ] }, server: { host: "127.0.0.1", port: 0 }, logLevel: "error" });
  await server.listen();
  browser = await (useWebKit ? webkit : chromium).launch(useWebKit ? { headless: true } : { headless: true, channel: "chrome" });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, reducedMotion: "reduce" });
  page.setDefaultTimeout(10000);
  const errors = [], requests = [], checks = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("request", request => { if (!request.url().startsWith(server.resolvedUrls.local[0])) requests.push(request.url()); });
  const graph = () => page.locator('[aria-label="3D 문서 참조 관계"]');
  const ready = async () => { await graph().waitFor(); await graph().scrollIntoViewIfNeeded(); await page.waitForFunction(() => { const g=window.__graphs.at(-1); return g?.renderer().info.render.frame > 45; }); };
  const choose = async id => page.getByLabel("3D 그래프 문서 선택").selectOption('wiki/'+id+'.md');
  await page.goto(server.resolvedUrls.local[0]+"index.html"); await ready();
  await page.evaluate(() => document.documentElement.classList.add("dark"));
  assert.equal(await page.getByRole("button", {name:"3D", exact:true}).getAttribute("aria-pressed"), "true");
  assert.equal(await page.evaluate(() => window.__graphs.at(-1).graphData().nodes.length), 24);
  // Click an actual rendered node using its screen projection, not a simulated callback.
  const point = await page.evaluate(() => { const g=window.__graphs.at(-1), n=g.graphData().nodes.find(n=>n.id==='wiki/1.md'); return g.graph2ScreenCoords(n.x,n.y,n.z); });
  const bounds = await graph().boundingBox();
  await page.mouse.move(bounds.x+point.x,bounds.y+point.y);
  await page.waitForFunction(() => window.__hover==='wiki/1.md');
  await page.mouse.click(bounds.x+point.x,bounds.y+point.y);
  await page.waitForFunction(() => document.querySelector('article h2')?.textContent === '문서 1');
  for (const id of [4,8,12,16,20,1]) await choose(id);
  assert.equal(await page.evaluate(() => window.__graphs.length), 1);
  checks.push("real WebGL node click and repeated selection keep one renderer");
  await choose(2);
  assert.equal(await page.locator("article h2").textContent(), '<img src=x onerror="window.XSS=1">');
  assert.equal(await page.evaluate(() => window.XSS), undefined);
  await page.getByRole("button", { name:"2D", exact:true }).click();
  assert.equal(await page.locator('g[aria-pressed="true"]').getAttribute("aria-label"), '그래프 문서: <img src=x onerror="window.XSS=1">');
  await page.getByRole("button", { name:"3D", exact:true }).click(); await ready();
  await page.getByLabel("위키 태그", { exact:true }).selectOption("개발");
  await page.waitForFunction(() => window.__graphs.at(-1).graphData().nodes.length===12);
  await page.getByLabel("위키 문서 검색").fill("문서 1");
  await page.waitForFunction(() => window.__graphs.at(-1).graphData().nodes.length===6);
  await page.getByLabel("위키 문서 검색").fill("");
  await page.getByLabel("위키 태그", { exact:true }).selectOption("");
  await choose(0);
  await page.getByLabel("그래프 연결 범위").selectOption("1");
  await page.waitForFunction(() => window.__graphs.at(-1).graphData().nodes.length===4);
  await page.getByLabel("그래프 연결 범위").selectOption("all");
  checks.push("2D/3D selection, safe titles, search, tags and depth share state");
  await page.getByRole("button", { name:"원문 편집", exact:true }).click();
  await page.getByLabel("문서 원문").fill("# 수정한 문서\n\n작성 중 내용");
  await choose(1);
  assert.equal(await page.getByLabel("문서 원문").inputValue(), "# 수정한 문서\n\n작성 중 내용");
  assert.equal(await page.getByLabel("3D 그래프 문서 선택").inputValue(), "wiki/0.md");
  await page.getByRole("button", { name:"문서 저장", exact:true }).click();
  await page.waitForFunction(() => window.__graphs.at(-1).graphData().nodes.find(n=>n.id==='wiki/0.md')?.title==='수정한 문서');
  checks.push("unsaved edit protected and saved title refreshed in 3D");
  await page.getByRole("button", { name:"화면 맞춤", exact:true }).click();
  await page.getByLabel("그래프 확대", {exact:true}).click();
  await page.getByLabel("그래프 축소", {exact:true}).click();
  await page.screenshot({ path:path.join(directory,"desktop-dark.png"), fullPage:true });
  const frameBeforeResize = await page.evaluate(() => window.__graphs.at(-1).renderer().info.render.frame);
  await page.setViewportSize({width:375,height:850});
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await graph().scrollIntoViewIfNeeded();
  await page.waitForFunction(previous => { const g=window.__graphs.at(-1); return g.width()<375 && g.renderer().info.render.frame>previous+2; }, frameBeforeResize);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth>innerWidth),false);
  await page.screenshot({ path:path.join(directory,"narrow-light.png"), fullPage:true });
  checks.push("theme and narrow resize with no page overflow");
  // Exercise the actual context loss event and verify the reading/editing surface survives.
  await page.evaluate(() => window.__graphs.at(-1).renderer().getContext().getExtension('WEBGL_lose_context').loseContext());
  await page.getByText("3D 그래프를 표시할 수 없어 2D로 전환했습니다.", {exact:false}).waitFor();
  assert.equal(await page.locator("article h2").textContent(), "수정한 문서");
  await page.getByRole("button", { name:"원문 편집", exact:true }).click();
  assert.ok(await page.getByLabel("문서 원문").isVisible());
  checks.push("actual WebGL context loss recovers to readable and editable 2D");
  await page.goto(server.resolvedUrls.local[0]+"index.html?count=201"); await ready();
  await page.locator('nav button').last().click();
  await graph().scrollIntoViewIfNeeded();
  await page.waitForFunction(() => { const n=window.__graphs.at(-1).graphData().nodes; return n.length===200 && n.some(n=>n.id==='wiki/200.md'); });
  for(let i=0;i<3;i++) { await page.getByRole("button",{name:"2D",exact:true}).click(); await page.getByRole("button",{name:"3D",exact:true}).click(); await ready(); }
  assert.equal(await page.evaluate(() => window.__disposed),3);
  await page.getByRole("button",{name:"그래프 접기",exact:true}).click();
  assert.equal(await page.evaluate(() => window.__disposed),4);
  checks.push("200-node cap retains selected document; repeated switch and collapse dispose renderers");
  await page.goto(server.resolvedUrls.local[0]+"index.html?count=1"); await ready();
  assert.equal(await page.evaluate(() => { const p=window.__graphs.at(-1).cameraPosition(); return [p.x,p.y,p.z].every(Number.isFinite); }),true);
  await page.evaluate(() => window.__graphs.at(-1).pauseAnimation());
  await page.getByText("3D 그래프를 표시할 수 없어 2D로 전환했습니다.", {exact:false}).waitFor();
  assert.equal(await page.locator("article h2").textContent(), "문서 0");
  checks.push("single node stays finite and stopped rendering recovers automatically");
  assert.deepEqual(errors,[]); assert.deepEqual(requests,[]);
  await writeFile(path.join(directory,"browser.json"),JSON.stringify({result:"pass",environment:`${useWebKit ? "Playwright WebKit" : "Google Chrome"} WebGL; real components and renderer, fixture IPC`,checks,errors,externalRequests:requests},null,2)+"\n");
  console.log(JSON.stringify({result:"pass",checks,errors,externalRequests:requests}));
} finally {
  if(browser) await browser.close();
  if(server) await server.close();
  await rm(fixture,{recursive:true,force:true});
}
