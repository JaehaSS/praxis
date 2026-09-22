use std::path::Path;

use crate::managed_process::SharedProcessRegistrar;
use crate::verify::{self, ValidateSpec, VerifyReport};

#[derive(Default)]
pub(super) struct VerifyRegistrars {
    pub(super) build: Option<SharedProcessRegistrar>,
    pub(super) test: Option<SharedProcessRegistrar>,
}

pub(super) fn execute(
    root: &Path,
    spec: ValidateSpec,
    registrars: VerifyRegistrars,
) -> VerifyReport {
    let build = spec.build.as_ref().map(|command| {
        verify::run_check_registered(root, command, spec.timeout_secs, registrars.build.as_ref())
    });
    let test = spec.test.as_ref().map(|command| {
        verify::run_check_registered(root, command, spec.timeout_secs, registrars.test.as_ref())
    });
    let summary = test
        .as_ref()
        .and_then(|result| verify::parse_test_summary(&result.tail));
    let gate = verify::gate(&verify::EvidenceBundle {
        build: build.clone(),
        tests: test.clone(),
        test_summary: summary,
        changed_files: vec![],
        created_at: 0,
    });
    VerifyReport {
        spec,
        build,
        test,
        summary,
        ready: gate.ready,
        checks: gate.checks,
        warnings: gate.warnings,
    }
}
