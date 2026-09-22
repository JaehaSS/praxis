use super::*;
use crate::convo::{turn_guard::SubagentTurnGuard, ConvoEvent, Vendor};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Stdio};

const FIXTURE_TEST: &str = "convo::process_cleanup::runtime_tests::codex_fixture";
const ROLE: &str = "PRAXIS_TEST_CODEX_ROLE";
const ROOT: &str = "PRAXIS_TEST_CODEX_ROOT";
const CASE: &str = "PRAXIS_TEST_CODEX_CASE";

fn fixture_command(path: impl AsRef<std::ffi::OsStr>, role: &str) -> Command {
    let mut command = Command::new(path);
    command
        .args(["--exact", FIXTURE_TEST, "--nocapture"])
        .env(ROLE, role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// Run copies of this native test binary so OS executable paths, rather than
/// shell script paths or forged argv, participate in the production predicate.
#[test]
fn codex_fixture() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let root = PathBuf::from(std::env::var(ROOT).unwrap());
    let case = std::env::var(CASE).unwrap();
    let mut children = Vec::new();
    if role == "wrapper" || (role == "vendor" && case == "nested-codex") {
        let role = if role == "wrapper" {
            "vendor"
        } else {
            "inner-vendor"
        };
        children.push(fixture_command(root.join("codex"), role).spawn().unwrap());
    } else if role == "vendor" || role == "inner-vendor" {
        let name = match case.as_str() {
            "other-directory" => "other/codex-code-mode-host",
            "forged-argv" => "job",
            _ => "codex-code-mode-host",
        };
        let mut command = fixture_command(root.join(name), "helper");
        command.process_group(0);
        if case == "forged-argv" {
            command.arg0(root.join("codex-code-mode-host"));
        }
        children.push(command.spawn().unwrap());
    } else if role == "helper" {
        if matches!(case.as_str(), "same-group-job" | "detached-job") {
            let mut command = fixture_command(root.join("job"), "job");
            if case == "detached-job" {
                command.process_group(0);
            }
            let child = command.spawn().unwrap();
            std::fs::write(root.join("job.pid"), child.id().to_string()).unwrap();
            children.push(child);
        }
        std::fs::write(root.join("helper.pid"), std::process::id().to_string()).unwrap();
    }
    // The parent test owns cleanup even on assertion failure. Bound the fixture
    // lifetime as an additional backstop, and reap direct children on normal exit.
    std::thread::sleep(Duration::from_secs(15));
    for mut child in children {
        child.kill().ok();
        child.wait().ok();
    }
}

struct Fixture {
    root: PathBuf,
    scope: TurnProcessScope,
    vendor: Child,
}

impl Fixture {
    fn start(case: &str) -> Self {
        let root = std::env::temp_dir().join(format!("praxis-runtime-helper-{}", unique_token()));
        std::fs::create_dir_all(root.join("other")).unwrap();
        let executable = std::env::current_exe().unwrap();
        let mut names = vec!["codex", "codex-code-mode-host"];
        match case {
            "other-parent" => names[0] = "other-vendor",
            "other-directory" => names[1] = "other/codex-code-mode-host",
            "forged-argv" => names[1] = "job",
            "same-group-job" | "detached-job" => names.push("job"),
            "node-wrapper" => names.push("node"),
            _ => {}
        }
        for name in names {
            // Distinct files are essential: macOS proc_pidpath can report the
            // same alias for both executables when they share a hard-linked inode.
            std::fs::copy(&executable, root.join(name)).unwrap();
        }
        let vendor_name = match case {
            "other-parent" => "other-vendor",
            "node-wrapper" => "node",
            _ => "codex",
        };
        let role = if case == "node-wrapper" {
            "wrapper"
        } else {
            "vendor"
        };
        let mut command = fixture_command(root.join(vendor_name), role);
        command.env(ROOT, &root).env(CASE, case).process_group(0);
        let scope = TurnProcessScope::attach(&mut command);
        let vendor = command.spawn().unwrap();
        Self {
            root,
            scope,
            vendor,
        }
    }

    fn pid(&self, name: &str) -> u32 {
        // The wrapper starts three separate native images. macOS cold startup
        // approached the old 3s bound even without test contention.
        for _ in 0..1000 {
            if let Some(pid) = std::fs::read_to_string(self.root.join(name))
                .ok()
                .and_then(|text| text.parse().ok())
            {
                return pid;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("native fixture did not become ready within 10s: {name}");
    }

    fn survivors(&self, vendor: Vendor) -> Vec<Survivor> {
        self.scope.survivors(self.vendor.id(), vendor)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.scope.cleanup();
        self.vendor.kill().ok();
        self.vendor.wait().ok();
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn assert_guard_result(survivors: &[Survivor], expected_error: bool) {
    let event = ConvoEvent::Result {
        text: "done".into(),
        is_error: false,
        session_id: "fixture".into(),
        cost_usd: 0.0,
        num_turns: 1,
        tokens_in: 0,
        tokens_out: 0,
    };
    let guarded = SubagentTurnGuard::default().enforce_result_with_survivors(event, survivors);
    assert!(matches!(guarded, ConvoEvent::Result { is_error, .. } if is_error == expected_error));
}

#[test]
fn a_trusted_codex_helper_is_not_a_job_but_is_still_cleaned_up() {
    let fixture = Fixture::start("helper-only");
    let helper = fixture.pid("helper.pid");
    assert_eq!(process_parent(helper), Some(fixture.vendor.id()));
    let survivors = fixture.survivors(Vendor::Codex);
    assert!(survivors.is_empty(), "{survivors:?}");
    assert_guard_result(&survivors, false);
    assert_eq!(fixture.survivors(Vendor::Claude)[0].group, helper);
    assert!(marked_process_groups(fixture.scope.marker()).contains(&helper));
    let mut reader = MarkerReader::new();
    assert!(!is_codex_runtime_helper(
        helper,
        fixture.vendor.id(),
        b"WRONG_TURN=1",
        &mut reader
    ));
    assert!(!is_codex_runtime_helper(
        helper,
        helper,
        fixture.scope.marker().as_bytes(),
        &mut reader
    ));

    assert!(
        fixture.scope.cleanup(),
        "cleanup must include the trusted helper"
    );
    assert!(marked_process_groups(fixture.scope.marker()).is_empty());
}

#[test]
fn the_npm_node_wrapper_is_part_of_the_top_level_codex_runtime() {
    let fixture = Fixture::start("node-wrapper");
    let helper = fixture.pid("helper.pid");
    let codex = process_parent(helper).unwrap();
    assert_ne!(codex, fixture.vendor.id());
    assert_eq!(process_parent(codex), Some(fixture.vendor.id()));
    let survivors = fixture.survivors(Vendor::Codex);
    assert!(survivors.is_empty(), "{survivors:?}");
    assert_guard_result(&survivors, false);
}

#[test]
fn helpers_do_not_hide_jobs_in_their_own_or_a_detached_group() {
    for case in ["same-group-job", "detached-job"] {
        let fixture = Fixture::start(case);
        let helper = fixture.pid("helper.pid");
        let job = fixture.pid("job.pid");
        let survivors = fixture.survivors(Vendor::Codex);
        assert_guard_result(&survivors, true);
        assert_eq!(survivors.len(), 1, "{case}: {survivors:?}");
        assert_eq!(
            survivors[0].group,
            if case == "same-group-job" {
                helper
            } else {
                job
            }
        );
        assert!(survivors[0].arguments.as_ref().unwrap()[0].ends_with("/job"));
    }
}

#[test]
fn a_name_match_without_the_installed_sibling_and_parent_is_not_exempt() {
    for case in [
        "forged-argv",
        "other-directory",
        "other-parent",
        "nested-codex",
    ] {
        let fixture = Fixture::start(case);
        let helper = fixture.pid("helper.pid");
        let survivors = fixture.survivors(Vendor::Codex);
        assert_guard_result(&survivors, true);
        assert_eq!(survivors.len(), 1, "{case}: {survivors:?}");
        assert_eq!(survivors[0].group, helper);
    }
}

#[test]
fn linux_parent_parser_handles_comm_parentheses_and_unreadable_processes_stay_unknown() {
    assert_eq!(
        parse_proc_parent("123 (helper (worker)) S 42 123 0"),
        Some(42)
    );
    for bad in [
        "",
        "123 (helper)",
        "123 (helper) S invalid",
        "123 helper S 42",
    ] {
        assert_eq!(parse_proc_parent(bad), None);
    }
    assert_eq!(process_parent(u32::MAX), None);
}
