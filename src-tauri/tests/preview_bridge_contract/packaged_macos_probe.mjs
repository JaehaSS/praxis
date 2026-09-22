import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";

const fixtureDir = new URL("./", import.meta.url);
const appPath = process.env.PRAXIS_PREVIEW_PROBE_APP;

function strictPayload() {
  const empty = JSON.stringify({ probe: "strict-csp", bytes: "" });
  return JSON.stringify({ probe: "strict-csp", bytes: "x".repeat(512 * 1024 - Buffer.byteLength(empty)) });
}

function validateEvidence(value) {
  if (value.transport !== "ipc") throw new Error("probe did not use IPC");
  if (value.payloadBytes !== 512 * 1024) throw new Error("probe did not send 512KiB payload");
  if (!/^[a-f0-9]{64}$/.test(value.sha256)) throw new Error("probe SHA-256 is invalid");
  if (value.aclDenied !== true) throw new Error("remote app command was not denied");
  if (value.taskId !== -4242) throw new Error("probe task binding is invalid");
  if (value.webviewLabel !== "designmode-probe") throw new Error("probe webview binding is invalid");
  if (!/^[a-f0-9]{32}$/.test(value.commandId)) throw new Error("probe command ID is invalid");
  if (value.fallback?.active || value.fallback?.executed) throw new Error("fallback ran after IPC pass");
  if (typeof value.elapsedMs !== "number") throw new Error("probe elapsed time is missing");
  return value;
}

async function startFixtureServer() {
  const server = createServer(async (request, response) => {
    const file = request.url === "/strict-csp-probe-client.js"
      ? "strict-csp-probe-client.js"
      : "strict_csp_preview.html";
    const content = await readFile(new URL(file, fixtureDir));
    response.setHeader("content-type", file.endsWith(".js") ? "text/javascript" : "text/html");
    response.end(content);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return server;
}

function waitForEvidence(child) {
  return new Promise((resolve, reject) => {
    let stdout = "";
    const timeout = setTimeout(() => reject(new Error("packaged probe timed out")), 10_000);
    child.once("error", reject);
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.once("exit", (code) => {
      clearTimeout(timeout);
      if (code !== 0) return reject(new Error(`packaged probe exited ${code}`));
      const line = stdout.trim().split("\n").find((item) => item.startsWith("{"));
      if (!line) return reject(new Error("packaged probe emitted no evidence"));
      try {
        resolve(validateEvidence(JSON.parse(line)));
      } catch (error) {
        reject(error);
      }
    });
  });
}

if (!appPath) throw new Error("PRAXIS_PREVIEW_PROBE_APP is required");
const server = await startFixtureServer();
const port = server.address().port;
let child;
try {
  child = spawn(appPath, [], {
    env: { ...process.env, PRAXIS_PREVIEW_PROBE_URL: `http://127.0.0.1:${port}/strict_csp_preview.html` },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const evidence = await waitForEvidence(child);
  const payload = strictPayload();
  if (Buffer.byteLength(payload) !== 512 * 1024) throw new Error("runner payload is not 512KiB");
  const expected = createHash("sha256").update(payload).digest("hex");
  if (evidence.sha256 !== expected) throw new Error("probe evidence SHA does not match strict payload");
  process.stdout.write(`${JSON.stringify(evidence)}\n`);
} finally {
  if (child) child.kill();
  await new Promise((resolve) => server.close(resolve));
}
