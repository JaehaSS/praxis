//! 읽기 전용 하네스 경험 문서 조회.

mod discovery;
mod model;
mod read;
mod registration;
mod result;

pub use discovery::list;
pub use model::{
    DocumentDescriptor, DocumentOwner, ExperienceDocument, HarnessName, Host, ProjectRef,
    ReadError, Relation, Scope, SourceRef, SourceResolution, Vendor,
};
pub use read::read;
pub use registration::registered_root;
pub use result::{
    ListResult, ProjectListing, ReadErrorOrRequest, ReadResult, RequestError, SourceListing,
    UnsupportedReason,
};

#[cfg(all(test, unix))]
#[path = "experience/fifo_tests.rs"]
mod fifo_tests;
#[cfg(test)]
#[path = "experience/matrix_tests.rs"]
mod matrix_tests;
#[cfg(test)]
#[path = "experience/project_tests.rs"]
mod project_tests;
#[cfg(test)]
mod tests;
