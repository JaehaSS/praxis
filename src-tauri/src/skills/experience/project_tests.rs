use super::matrix_tests::{fixture, owner, write};
use super::*;
use std::fs;

#[test]
fn limits_project_markdown_and_keeps_ordinary_lessons_unconfirmed() {
    let project = fixture("project");
    let home = fixture("project-home");
    write(&project, "LESSONS_LEARNED.md", "ordinary");
    write(&project, "docs/lessons-seed.md", "seed");
    fs::create_dir_all(project.join("docs/archive")).unwrap();
    for index in 0..201 {
        write(&project, &format!("docs/memory/{index}.md"), "item");
    }
    let ListResult::Ready {
        project: listing, ..
    } = list(&project, &home, HarnessName::WorkflowHarness)
    else {
        panic!("ready")
    };
    assert!(matches!(
        listing,
        ProjectListing::Ready {
            limited: true,
            inspected_entries: 201,
            ..
        }
    ));
    let ReadResult::Ready { document } = read(&project, &home, owner(&project), "project-lessons")
    else {
        panic!("read")
    };
    assert!(!document.generated);
    assert_eq!(document.source_resolution, SourceResolution::NotApplicable);
}

#[cfg(unix)]
#[test]
fn reports_symlink_project_documents_as_unsafe_for_list_and_read() {
    let project = fixture("symlink");
    let home = fixture("symlink-home");
    let outside = fixture("outside").join("outside.md");
    fs::write(&outside, "outside").unwrap();
    fs::create_dir_all(project.join("docs/memory")).unwrap();
    std::os::unix::fs::symlink(&outside, project.join("docs/memory/link.md")).unwrap();
    let ListResult::Ready {
        project: listing, ..
    } = list(&project, &home, HarnessName::WorkflowHarness)
    else {
        panic!("ready")
    };
    assert!(matches!(
        listing,
        ProjectListing::Error {
            error: ReadError::UnsafePath,
            ..
        }
    ));
    assert!(matches!(
        read(&project, &home, owner(&project), "memory/link.md"),
        ReadResult::Error {
            error: ReadErrorOrRequest::Read(ReadError::UnsafePath)
        }
    ));
}
