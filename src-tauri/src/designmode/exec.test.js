// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, it, expect, beforeEach } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = readFileSync(path.join(here, "exec.js"), "utf-8");
beforeEach(() => { document.body.innerHTML = ""; new Function(source)(); });

describe("snapshot", () => {
  it("lists interactive elements with role, name and generation refs", () => {
    document.body.innerHTML = `<h1>대시보드</h1><input aria-label="이메일"><button>로그인</button>`;
    const snap = window.__praxisPreviewExec.snapshot();
    expect(snap.generation).toBe(1);
    expect(snap.text).toContain('heading "대시보드" level=1');
    expect(snap.text).toMatch(/textbox "이메일" \[ref=s1e\d+\]/);
    expect(snap.text).toMatch(/button "로그인" \[ref=s1e\d+\]/);
  });
  it("increments generation per snapshot and rejects stale refs", () => {
    document.body.innerHTML = `<button>확인</button>`;
    const first = window.__praxisPreviewExec.snapshot();
    const ref = first.text.match(/ref=(s1e\d+)/)[1];
    window.__praxisPreviewExec.snapshot();
    expect(window.__praxisPreviewExec.resolve(ref)).toEqual({ error: "stale_ref" });
  });
  it("truncates deep trees and flags it", () => {
    let html = "x"; for (let i = 0; i < 900; i++) html = `<div><button>b${i}</button>${html}</div>`;
    document.body.innerHTML = html;
    const snap = window.__praxisPreviewExec.snapshot({ maxNodes: 800 });
    expect(snap.truncated).toBe(true);
  });
  it("skips only the subtree that exceeds maxDepth and keeps later siblings", () => {
    let deep = `<button>깊은</button>`;
    for (let i = 0; i < 20; i++) deep = `<div>${deep}</div>`;
    document.body.innerHTML = `${deep}<button>하나</button><button>둘</button><button>셋</button>`;
    const snap = window.__praxisPreviewExec.snapshot();
    expect(snap.truncated).toBe(true);
    expect(snap.text).not.toContain(`"깊은"`);
    for (const name of ["하나", "둘", "셋"]) expect(snap.text).toContain(`button "${name}"`);
  });
  it("stops the whole traversal once maxNodes is reached", () => {
    document.body.innerHTML = Array.from({ length: 900 }, (_, i) => `<button>b${i}</button>`).join("");
    const snap = window.__praxisPreviewExec.snapshot({ maxNodes: 800 });
    expect(snap.text.split("\n")).toHaveLength(800);
    expect(snap.truncated).toBe(true);
  });
  it("emits no value for password or one-time-code inputs", () => {
    document.body.innerHTML =
      `<input type="password" aria-label="비밀번호" value="hunter2">` +
      `<input autocomplete="one-time-code" aria-label="인증코드" value="123456">`;
    const snap = window.__praxisPreviewExec.snapshot();
    expect(snap.text).toContain("type=password");
    expect(snap.text).toContain("type=otp");
    expect(snap.text).not.toContain("value=");
    expect(snap.text).not.toContain("hunter2");
    expect(snap.text).not.toContain("123456");
  });
  it("emits no value for hidden inputs", () => {
    document.body.innerHTML = `<input type="hidden" name="csrf" value="tok123">`;
    const snap = window.__praxisPreviewExec.snapshot();
    expect(snap.text).toContain("type=hidden");
    expect(snap.text).not.toContain("tok123");
  });
  it("caps other values at 80 code points", () => {
    document.body.innerHTML = `<input aria-label="메모">`;
    document.querySelector("input").value = "z".repeat(200);
    const snap = window.__praxisPreviewExec.snapshot();
    expect(snap.text).toContain(`value="${"z".repeat(80)}"`);
    expect(snap.text).not.toContain("z".repeat(81));
  });
  it("slices names by code point so surrogate pairs stay intact", () => {
    document.body.innerHTML = `<button aria-label="${"a".repeat(79)}🚀 나머지 텍스트">확인</button>`;
    const snap = window.__praxisPreviewExec.snapshot();
    expect(snap.text).toContain(`"${"a".repeat(79)}🚀"`);
    expect(snap.text).not.toContain("나머지");
    expect(JSON.parse(JSON.stringify(snap.text))).toBe(snap.text);
    expect(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/.test(snap.text)).toBe(false);
  });
  it("resolves a live ref and reports stale_ref once the element leaves the DOM", () => {
    document.body.innerHTML = `<button>확인</button>`;
    const button = document.querySelector("button");
    const snap = window.__praxisPreviewExec.snapshot();
    const ref = snap.text.match(/ref=(s1e\d+)/)[1];
    expect(window.__praxisPreviewExec.resolve(ref)).toEqual({ el: button });
    button.remove();
    expect(window.__praxisPreviewExec.resolve(ref)).toEqual({ error: "stale_ref" });
  });
});

describe("run", () => {
  it("dispatches snapshot commands and returns a JSON string body", async () => {
    document.body.innerHTML = `<button>확인</button>`;
    const body = await window.__praxisPreviewExec.run({ op: "snapshot" });
    const parsed = JSON.parse(body);
    expect(parsed.ok).toBe(true);
    expect(parsed.snapshot.text).toContain("button");
  });
  it("halves limits and marks truncated on payload_too_large", async () => {
    let html = "x"; for (let i = 0; i < 300; i++) html = `<div><button>${"한".repeat(50)}${i}</button>${html}</div>`;
    document.body.innerHTML = html;
    const maxBytes = 4096;
    const body = await window.__praxisPreviewExec.run({ op: "snapshot", maxBytes });
    expect(JSON.parse(body).snapshot.truncated).toBe(true);
    expect(new TextEncoder().encode(body).length).toBeLessThanOrEqual(maxBytes);
  });
  it("rejects unknown ops", async () => {
    expect(await window.__praxisPreviewExec.run({ op: "scroll" })).toBe(`{"ok":false,"error":"unknown_op"}`);
  });
  it("measures the tree body in bytes and nodes, not the whole response", async () => {
    document.body.innerHTML = `<button>확인</button><input aria-label="이메일">`;
    const { snapshot } = JSON.parse(await window.__praxisPreviewExec.run({ op: "snapshot" }));
    expect(snapshot.nodes).toBe(2);
    expect(snapshot.bytes).toBe(new TextEncoder().encode(snapshot.text).length);
    expect(snapshot.shrinks).toBe(0);
  });
  it("counts Hangul as three bytes so the cost is not under-measured", async () => {
    document.body.innerHTML = `<button>확인</button>`;
    const { snapshot } = JSON.parse(await window.__praxisPreviewExec.run({ op: "snapshot" }));
    expect(snapshot.bytes).toBeGreaterThan(snapshot.text.length);
  });
  // 깊은 트리는 maxDepth에서 먼저 잘려 축소까지 가지 않는다 — 넓고 얕게 만들어야 상한에 걸린다.
  it("reports how many times the limits were halved", async () => {
    document.body.innerHTML = Array.from(
      { length: 800 }, (_, i) => `<button>${"가".repeat(20)}${i}</button>`).join("");
    const full = await window.__praxisPreviewExec.run({ op: "snapshot" }).then(JSON.parse);
    expect(full.snapshot.shrinks).toBe(0);

    // 온전한 본문과 그 절반 사이에 상한을 두면 축소가 정확히 한 번 일어난다.
    const { snapshot } = await window.__praxisPreviewExec
      .run({ op: "snapshot", maxBytes: Math.floor(full.snapshot.bytes * 0.75) }).then(JSON.parse);
    expect(snapshot.shrinks).toBe(1);
    expect(snapshot.truncated).toBe(true);
    expect(snapshot.nodes).toBe(400);
  });
  it("snapshots an empty body as empty text", async () => {
    document.body.innerHTML = "";
    const parsed = JSON.parse(await window.__praxisPreviewExec.run({ op: "snapshot" }));
    expect(parsed.ok).toBe(true);
    expect(parsed.snapshot.text).toBe("");
  });
  it("gives up with payload_too_large after two attempts", async () => {
    document.body.innerHTML = `<button>확인</button>`;
    const body = await window.__praxisPreviewExec.run({ op: "snapshot", maxBytes: 10 });
    expect(JSON.parse(body)).toEqual({ ok: false, error: "payload_too_large" });
  });
});
