#![cfg(unix)]
#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::convo::{run_turn_with_effort, Vendor};
use std::os::unix::fs::PermissionsExt;

#[test]
fn exec_passes_speed_on_new_and_resumed_turns() {
    let dir = temp_root::dir().join(format!("praxis-speed-exec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("provider");
    std::fs::write(
        &bin,
        r#"#!/usr/bin/python3
import sys,json
with open('argv.json','w') as f: json.dump(sys.argv[1:],f)
print(json.dumps({'type':'thread.started','thread_id':'speed-thread'}),flush=True)
print(json.dumps({'type':'turn.completed','usage':{'input_tokens':1,'output_tokens':1}}),flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    for tier in [None, Some("fast"), Some("default")] {
        for resume in [None, Some("speed-thread")] {
            let outcome = run_turn_with_effort(
                dir.to_str().unwrap(),
                "hi",
                resume,
                5,
                Vendor::Codex,
                bin.to_str().unwrap(),
                Some("gpt-6-astra"),
                Some("high"),
                tier,
                &[],
                None,
                None,
                |_| {},
                |_| {},
            )
            .unwrap();
            assert_eq!(outcome.session_id, "speed-thread");
            let args: Vec<String> =
                serde_json::from_slice(&std::fs::read(dir.join("argv.json")).unwrap()).unwrap();
            assert_eq!(args[0], "exec");
            assert_eq!(args.iter().any(|arg| arg == "resume"), resume.is_some());
            let speed: Vec<_> = args
                .iter()
                .filter(|arg| arg.starts_with("service_tier="))
                .collect();
            if let Some(tier) = tier {
                assert_eq!(speed, vec![&format!("service_tier=\"{tier}\"")]);
            } else {
                assert!(speed.is_empty());
            }
            assert_eq!(
                args.iter().any(|arg| arg == "features.fast_mode=true"),
                tier == Some("fast")
            );
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
