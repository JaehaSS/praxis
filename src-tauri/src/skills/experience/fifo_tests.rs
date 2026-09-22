use super::*;
use std::ffi::CString;
use std::fs;

#[test]
fn excludes_fifo_project_entries_after_descriptor_enumeration() {
    let project = crate::testtmp::dir().join(format!("experience-fifo-{}", std::process::id()));
    let home = crate::testtmp::dir().join(format!("experience-fifo-home-{}", std::process::id()));
    fs::create_dir_all(project.join("docs/memory")).unwrap();
    fs::create_dir_all(&home).unwrap();
    let fifo = project.join("docs/memory/pipe.md");
    let name = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { nix::libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let ListResult::Ready {
        project: listing, ..
    } = list(&project, &home, HarnessName::WorkflowHarness)
    else {
        panic!("expected ready response")
    };
    assert!(matches!(listing, ProjectListing::Empty { .. }));
}
