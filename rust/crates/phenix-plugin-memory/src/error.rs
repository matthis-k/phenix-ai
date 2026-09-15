use std::fmt::{self, Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MemoryError {
    Invalid(String),
    Missing(String),
    Conflict(String),
    Persistence(String),
    Provider(String),
}

impl Display for MemoryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid memory value: {message}"),
            Self::Missing(message) => write!(formatter, "missing memory value: {message}"),
            Self::Conflict(message) => write!(formatter, "memory conflict: {message}"),
            Self::Persistence(message) => {
                write!(formatter, "memory persistence failure: {message}")
            }
            Self::Provider(message) => write!(formatter, "memory provider failure: {message}"),
        }
    }
}

pub(crate) type MemoryResult<T> = Result<T, MemoryError>;
