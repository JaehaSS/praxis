import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const binary = process.env.PRAXIS_PREVIEW_PROBE_APP;
if (!binary) throw new Error("PRAXIS_PREVIEW_PROBE_APP is required");
const root = await mkdtemp(join(tmpdir(), "praxis-preview-auto-open-"));
const server = createServer((request, response) => {
  if (request.url === "/probe.js") {
    response.writeHead(200, { "Content-Type": "application/javascript" });
    response.end('document.querySelector("button").addEventListener("click",()=>document.querySelector("h1").textContent="Activated successfully");');
  } else {
    response.writeHead(200, { "Content-Type": "text/html", "Content-Security-Policy": "default-src 'self'; connect-src 'none'" });
    response.end('<!doctype html><html><head><title>Preview auto-open probe</title></head><body><h1>Preview activation probe</h1><button>Activate test</button><script src="/probe.js"></script></body></html>');
  }
});
let child;
let exited;
let timer;
try {
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  child = spawn(binary, [], {
    env: { ...process.env, PRAXIS_PREVIEW_AUTO_OPEN_PROBE_URL: `http://127.0.0.1:${server.address().port}/`, PRAXIS_PREVIEW_AUTO_OPEN_PROBE_DIR: root },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr = (stderr + chunk).slice(-12000); });
  exited = new Promise((resolve, reject) => { child.once("exit", resolve); child.once("error", reject); });
  const timeout = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("probe deadline")), 60000); });
  const code = await Promise.race([exited, timeout]);
  if (code !== 0) throw new Error(`probe exit ${code}: ${stderr}`);
  const evidence = stdout.split("\n").filter((line) => line.startsWith("{")).map((line) => JSON.parse(line)).find((value) => value.probe === "preview-auto-open");
  if (!evidence) throw new Error(`missing evidence: ${stdout}\n${stderr}`);
  for (const key of ["firstOpen", "duplicateBusy", "snapshot", "click", "visible", "reuse", "takeoverPreserved", "tokenPreserved", "reopen", "taskIsolation", "externalUrlRejected", "externalOriginRejected", "cleanup"]) {
    if (evidence[key] !== true) throw new Error(`failed acceptance: ${key}`);
  }
  console.log(JSON.stringify(evidence, null, 2));
} finally {
  clearTimeout(timer);
  if (child && child.exitCode === null && child.signalCode === null) { child.kill("SIGKILL"); await exited.catch(() => undefined); }
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
  await rm(root, { recursive: true, force: true });
}
