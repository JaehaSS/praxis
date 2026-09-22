use std::fmt;

pub type RestoreResult<T> = Result<T, RestoreFailure>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreFailureKind {
    NotFound,
    Invalid,
    Conflict,
    Storage,
}

#[derive(Debug)]
pub enum RestoreFailure {
    NotFound(&'static str),
    Invalid(&'static str),
    Conflict,
    Storage(sqlx::Error),
}

impl RestoreFailure {
    pub fn kind(&self) -> RestoreFailureKind {
        match self {
            Self::NotFound(_) => RestoreFailureKind::NotFound,
            Self::Invalid(_) => RestoreFailureKind::Invalid,
            Self::Conflict => RestoreFailureKind::Conflict,
            Self::Storage(_) => RestoreFailureKind::Storage,
        }
    }
}

impl fmt::Display for RestoreFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) | Self::Invalid(message) => formatter.write_str(message),
            Self::Conflict => {
                formatter.write_str("메모리 version 또는 상태가 변경되어 복구하지 못했습니다")
            }
            Self::Storage(_) => formatter.write_str("메모리 버전 복원 저장소 오류"),
        }
    }
}

impl std::error::Error for RestoreFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for RestoreFailure {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(error)
    }
}

pub fn is_conflict(error: &RestoreFailure) -> bool {
    error.kind() == RestoreFailureKind::Conflict
}
