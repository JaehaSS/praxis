#[test]
fn strict_csp_fixture_and_packaged_probe_runner_preserve_evidence_contract() {
    let fixture = include_str!("strict_csp_preview.html");
    let runner = include_str!("packaged_macos_probe.mjs");

    assert!(fixture.contains("default-src 'self'; connect-src 'none'"));
    assert!(fixture.contains("strict-csp-probe-client.js"));
    let client = include_str!("strict-csp-probe-client.js");
    assert!(client.contains("payloadBytes !== 512 * 1024"));
    for key in [
        "transport",
        "elapsedMs",
        "payloadBytes",
        "sha256",
        "aclDenied",
        "fallback",
        "taskId",
        "commandId",
        "webviewLabel",
    ] {
        assert!(runner.contains(key), "missing evidence key: {key}");
    }
    assert!(runner.contains("finally"));
    assert!(runner.contains("server.close"));
    assert!(runner.contains("child.kill"));
    assert!(runner.contains("strictPayload"));
    assert!(runner.contains("code !== 0"));
}

#[test]
fn ipc_pass_evidence_keeps_fallback_inactive_and_unexecuted() {
    let runner = include_str!("packaged_macos_probe.mjs");

    assert!(runner.contains("fallback?.active || value.fallback?.executed"));
}
