use std::fmt;

use serde::Serialize;

pub type ConfirmApprovalResult<T> = Result<T, ConfirmApprovalFailure>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConfirmedApproval {
    pub version: i64,
    pub receipt_id: i64,
    pub already_approved: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmApprovalFailureKind {
    NotFound,
    Invalid,
    Conflict,
    Storage,
}

#[derive(Debug)]
pub enum ConfirmApprovalFailure {
    NotFound(&'static str),
    Invalid(&'static str),
    Conflict,
    Storage(anyhow::Error),
}

impl ConfirmApprovalFailure {
    pub fn kind(&self) -> ConfirmApprovalFailureKind {
        match self {
            Self::NotFound(_) => ConfirmApprovalFailureKind::NotFound,
            Self::Invalid(_) => ConfirmApprovalFailureKind::Invalid,
            Self::Conflict => ConfirmApprovalFailureKind::Conflict,
            Self::Storage(_) => ConfirmApprovalFailureKind::Storage,
        }
    }
}

impl fmt::Display for ConfirmApprovalFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) | Self::Invalid(message) => formatter.write_str(message),
            Self::Conflict => {
                formatter.write_str("메모리 version 또는 상태가 변경되어 승인하지 못했습니다")
            }
            Self::Storage(_) => formatter.write_str("메모리 원자 승인 저장소 오류"),
        }
    }
}

impl std::error::Error for ConfirmApprovalFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for ConfirmApprovalFailure {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(error.into())
    }
}

impl From<anyhow::Error> for ConfirmApprovalFailure {
    fn from(error: anyhow::Error) -> Self {
        Self::Storage(error)
    }
}
