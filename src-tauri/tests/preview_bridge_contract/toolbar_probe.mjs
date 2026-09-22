import { spawn } from "node:child_process";
import { createServer } from "node:http";

const appPath = process.env.PRAXIS_PREVIEW_TOOLBAR_PROBE_APP;

function validateEvidence(value) {
  if (!value.toolbarUrl?.includes("window=preview-toolbar&task=-4242")) {
    throw new Error("toolbar App URL did not preserve its query");
  }
  if (!value.pageUrl?.includes("toolbar_probe.html")) throw new Error("external child did not mount");
  if (value.toolbarLabel !== "previewbar-probe") throw new Error("toolbar label is wrong");
  if (value.pageLabel !== "designmode-probe") throw new Error("page label is wrong");
  if (value.rootWebview) throw new Error("probe window has a root webview");
  if (value.standaloneWebviewWindows !== 0) throw new Error("probe created a standalone webview window");
  if (value.toolbarAppCommandDenied !== true) throw new Error("toolbar generic app command was not denied");
  if (value.childLabels?.sort().join(",") !== "designmode-probe,previewbar-probe") {
    throw new Error("probe did not mount exactly the two child webviews");
  }
  return value;
}

async function startFixtureServer() {
  const server = createServer((_request, response) => {
    response.setHeader("content-type", "text/html");
    response.end("<!doctype html><title>toolbar probe</title><p>mounted</p>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return server;
}

function waitForEvidence(child) {
  return new Promise((resolve, reject) => {
    let stdout = "";
    const timeout = setTimeout(() => reject(new Error("toolbar probe timed out")), 15_000);
    child.once("error", reject);
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.once("exit", (code) => {
      clearTimeout(timeout);
      if (code !== 0) return reject(new Error(`toolbar probe exited ${code}`));
      const line = stdout.trim().split("\n").find((item) => item.startsWith("{\"toolbarUrl\""));
      if (!line) return reject(new Error("toolbar probe emitted no evidence"));
      try {
        resolve(validateEvidence(JSON.parse(line)));
      } catch (error) {
        reject(error);
      }
    });
  });
}

if (!appPath) throw new Error("PRAXIS_PREVIEW_TOOLBAR_PROBE_APP is required");
const server = await startFixtureServer();
const port = server.address().port;
let child;
try {
  child = spawn(appPath, [], {
    env: { ...process.env, PRAXIS_PREVIEW_TOOLBAR_PROBE_URL: `http://127.0.0.1:${port}/toolbar_probe.html` },
    stdio: ["ignore", "pipe", "pipe"],
  });
  process.stdout.write(`${JSON.stringify(await waitForEvidence(child))}\n`);
} finally {
  if (child) child.kill();
  await new Promise((resolve) => server.close(resolve));
}
