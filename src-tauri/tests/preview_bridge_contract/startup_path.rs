#[test]
fn production_entrypoint_connects_registration_pending_eval_and_submit_result() {
    let commands = concat!(
        include_str!("../../src/commands.rs"),
        // designmode/preview 커맨드는 commands/designmode.rs로 갈라졌다.
        include_str!("../../src/commands/designmode.rs"),
    );
    let injected = include_str!("../../src/designmode/preview_agent.js");

    assert!(commands.contains("SessionRegistration::new("));
    assert!(commands.contains("preview_bridge.begin("));
    assert!(commands.contains("webview.eval("));
    assert!(injected.contains("submitResult"));
    assert!(injected.contains("plugin:preview-bridge|submit_result"));
}

#[test]
fn navigation_prepares_generation_before_native_webview_navigate() {
    let commands = concat!(
        include_str!("../../src/commands.rs"),
        // designmode/preview 커맨드는 commands/designmode.rs로 갈라졌다.
        include_str!("../../src/commands/designmode.rs"),
    );
    let commands = commands.split_whitespace().collect::<String>();
    let navigate = commands.find("handle.webview.navigate(parsed)").unwrap();
    let prepare = commands[..navigate]
        .rfind("preview_bridge.prepare_navigation(id)")
        .unwrap_or(usize::MAX);

    assert!(
        prepare < navigate,
        "prepare navigation before native navigation"
    );
}
