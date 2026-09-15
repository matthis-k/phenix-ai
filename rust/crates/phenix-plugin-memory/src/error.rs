#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum MemoryError {
    #[error("invalid memory value: {0}")]
    Invalid(String),
    #[error("missing memory value: {0}")]
    Missing(String),
    #[error("memory conflict: {0}")]
    Conflict(String),
    #[error("memory persistence failure: {0}")]
    Persistence(String),
    #[error("memory provider failure: {0}")]
    Provider(String),
}

pub(crate) type MemoryResult<T> = Result<T, MemoryError>;
