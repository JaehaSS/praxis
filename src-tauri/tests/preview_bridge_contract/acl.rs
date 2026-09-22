use praxis_lib::preview_bridge::{
    sha256_hex, PendingAction, PreviewBridge, ResultEnvelope, SessionRegistration,
};
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[tauri::command]
fn designmode_close() -> &'static str {
    "must not be callable by remote preview"
}

#[test]
fn generated_plugin_acl_is_remote_only_and_does_not_change_app_command_acl() {
    let build_script = include_str!("../../build.rs");
    let capability: serde_json::Value = serde_json::from_str(include_str!(
        "../../capabilities/preview-bridge-remote.json"
    ))
    .unwrap();
    let default_capability = include_str!("../../capabilities/default.json");

    assert!(build_script.contains("InlinedPlugin"));
    assert!(build_script.contains("preview-bridge"));
    assert!(build_script.contains("submit_result"));
    assert_eq!(capability["local"], false);
    assert_eq!(capability["webviews"], serde_json::json!(["designmode-*"]));
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["preview-bridge:allow-submit-result"])
    );
    assert_eq!(
        capability["remote"]["urls"],
        serde_json::json!(["http://localhost:*", "http://127.0.0.1:*"])
    );
    assert!(!default_capability.contains("preview-bridge"));
}

#[test]
fn mock_runtime_remote_invoke_reaches_only_the_one_plugin_command() {
    let bridge = PreviewBridge::new();
    bridge
        .register(SessionRegistration::new(
            42,
            "designmode-42-1",
            "session-a",
            7,
        ))
        .unwrap();
    bridge
        .begin(PendingAction::new(42, "session-a", 7, "command-a"))
        .unwrap();
    let payload = "{}".to_string();
    let result = ResultEnvelope::new(
        42,
        "session-a",
        7,
        "command-a",
        sha256_hex(payload.as_bytes()),
        payload,
    );
    let app = tauri::test::mock_builder()
        .plugin(praxis_lib::preview_bridge::tauri_plugin_with(bridge))
        .invoke_handler(tauri::generate_handler![designmode_close])
        .build(tauri::generate_context!())
        .unwrap();
    let webview = WebviewWindowBuilder::new(
        &app,
        "designmode-42-1",
        WebviewUrl::External("http://localhost:1421/strict-csp.html".parse().unwrap()),
    )
    .build()
    .unwrap();

    let request = |cmd: &str, body: serde_json::Value| tauri::webview::InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "http://localhost:1421/strict-csp.html".parse().unwrap(),
        body: body.into(),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.into(),
    };
    let allowed = tauri::test::get_ipc_response(
        &webview,
        request(
            "plugin:preview-bridge|submit_result",
            serde_json::json!({ "result": result }),
        ),
    );
    assert!(
        allowed.is_ok(),
        "allowed plugin request must reach Accepted: {allowed:?}"
    );
    let denied =
        tauri::test::get_ipc_response(&webview, request("designmode_close", serde_json::json!({})));
    assert!(format!("{denied:?}").to_lowercase().contains("not allowed"));
}
