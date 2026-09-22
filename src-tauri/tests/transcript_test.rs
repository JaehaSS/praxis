//! 트랜스크립트 파서 테스트. `cargo test`

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::transcript::{self, ClaudeCodeParser, TranscriptParser};

static COUNTER: AtomicU32 = AtomicU32::new(0);

#[test]
fn parses_user_and_assistant_messages_skips_control() {
    let jsonl = r#"
{"type":"queue-operation","sessionId":"sess-abc","operation":"start","timestamp":1}
{"type":"user","sessionId":"sess-abc","message":{"role":"user","content":"implement auth with JWT"},"timestamp":2}
{"type":"assistant","sessionId":"sess-abc","message":{"role":"assistant","content":[{"type":"text","text":"I will add a JWT module."},{"type":"tool_use","name":"edit","input":{}}]},"timestamp":3}
{"broken json line
"#;
    let d = transcript::parse_jsonl_str(jsonl)
        .expect("parse")
        .expect("some digest");
    assert_eq!(d.session_id, "sess-abc");
    assert_eq!(
        d.message_count, 2,
        "user + assistant only (control/broken skipped)"
    );
    assert!(d.text.contains("implement auth with JWT"));
    assert!(d.text.contains("I will add a JWT module."));
}

#[test]
fn empty_or_control_only_returns_none() {
    let jsonl = r#"{"type":"queue-operation","sessionId":"s","operation":"x","timestamp":1}"#;
    assert!(transcript::parse_jsonl_str(jsonl).unwrap().is_none());
    assert!(transcript::parse_jsonl_str("").unwrap().is_none());
}

#[test]
fn projects_dir_encodes_cwd_with_slash_and_dot_to_dash() {
    // 존재하는 디렉터리여야 canonicalize 가능.
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = temp_root::dir().join(format!("praxis-tx-{}-{}", std::process::id(), n));
    let wt = base.join(".praxis").join("worktrees").join("feat-x");
    std::fs::create_dir_all(&wt).unwrap();

    let dir = ClaudeCodeParser::projects_dir(&wt).expect("projects_dir");
    let s = dir.to_string_lossy();
    // 마지막 컴포넌트는 canonicalize된 cwd를 /·. → - 로 치환한 것.
    assert!(s.contains("/.claude/projects/"), "under projects: {s}");
    assert!(
        s.contains("-praxis-worktrees-feat-x"),
        "encoded suffix: {s}"
    );
    assert!(
        !s.rsplit('/').next().unwrap().contains('.'),
        "no dots in encoded dir name"
    );

    // 해당 디렉터리에 트랜스크립트가 없으면 extract는 None.
    let parser = ClaudeCodeParser;
    assert!(parser.extract(&wt).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&base);
}
