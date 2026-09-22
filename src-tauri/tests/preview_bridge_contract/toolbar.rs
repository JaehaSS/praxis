#[test]
fn toolbar_probe_records_its_two_children_without_a_root_webview() {
    let source = include_str!("../../src/commands/preview_window.rs");

    assert!(source.contains("WindowBuilder::new(app, TOOLBAR_PROBE_WINDOW_LABEL)"));
    assert!(source.contains("index.html?window=preview-toolbar&task=-4242"));
    assert!(source.contains("WebviewUrl::External(page_url)"));
    assert!(source.contains(".webviews()"));
    assert!(source.contains("get_webview_window(TOOLBAR_PROBE_WINDOW_LABEL)"));
    assert!(source.contains("app.webview_windows().len()"));
    assert!(source.contains("toolbar_probe_denial_script"));
    assert!(source.contains("toolbar_app_command_denied"));
}

#[test]
fn toolbar_acl_exposes_only_the_relay_and_state_listener_surface() {
    let build = include_str!("../../build.rs");
    let capability = include_str!("../../capabilities/preview-toolbar.json");
    let main_capability = include_str!("../../capabilities/preview-toolbar-main.json");
    let plugin = include_str!("../../src/preview_workbench/plugin.rs");
    let app = include_str!("../../src/lib.rs");

    assert!(build.contains("preview-workbench"));
    assert!(build.contains("commands(&[\"relay\", \"publish\"])"));
    assert!(capability.contains("previewbar-*"));
    assert!(capability.contains("preview-workbench:allow-relay"));
    assert!(!capability.contains("allow-emit"));
    assert!(main_capability.contains(r#""webviews": ["main"]"#));
    assert!(main_capability.contains("preview-workbench:allow-publish"));
    assert!(plugin.contains("EventTarget::Webview"));
    assert!(app.contains("label().starts_with(\"previewbar-\")"));
    assert!(app.contains("invoke.resolver.reject(\"not allowed\")"));
    assert!(plugin.contains("preview-workbench://toolbar-request"));
    assert!(plugin.contains("preview-workbench://toolbar-state"));
}
