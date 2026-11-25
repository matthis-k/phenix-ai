use crate::Key;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionRole {
    Offer,
    Requirement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigurationError {
    OwnerIdentifier {
        length: usize,
        limit: usize,
    },
    ContributionIdentifier {
        owner: String,
        length: usize,
        limit: usize,
    },
    ContributionType {
        key: Key,
        expected: &'static str,
    },
    DuplicateContribution {
        key: Key,
    },
    MissingDefinition {
        key: Key,
        contract: Key,
        role: DefinitionRole,
    },
    UnknownRequirement {
        requirement: Key,
    },
    UnknownOffer {
        offer: Key,
    },
    InvalidConsumer {
        requirement: Key,
        consumer: Key,
    },
    DuplicateDependency {
        contribution: Key,
        dependency: Key,
    },
    MissingDependency {
        contribution: Key,
        dependency: Key,
    },
    RoutesWithoutProducer {
        contribution: Key,
    },
    UnknownRoute {
        contribution: Key,
        route: String,
    },
    DuplicateRoute {
        contribution: Key,
        route: String,
    },
}
impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerIdentifier { .. } => {
                f.write_str("plugin owner identifier exceeds size limit")
            }
            Self::ContributionIdentifier { .. } => {
                f.write_str("plugin contribution identifier exceeds size limit")
            }
            Self::ContributionType { key, .. } => write!(f, "contribution type mismatch: {key:?}"),
            Self::DuplicateContribution { key } => write!(f, "duplicate contribution: {key:?}"),
            Self::MissingDefinition {
                key,
                contract,
                role,
            } => {
                let role = match role {
                    DefinitionRole::Offer => "offer",
                    DefinitionRole::Requirement => "requirement",
                };
                write!(
                    f,
                    "missing contract definition for {role} {key:?}: {contract:?}"
                )
            }
            Self::UnknownRequirement { requirement } => {
                write!(f, "host policy names unknown requirement: {requirement:?}")
            }
            Self::UnknownOffer { offer } => write!(f, "host policy names unknown offer: {offer:?}"),
            Self::InvalidConsumer {
                requirement,
                consumer,
            } => write!(
                f,
                "requirement {requirement:?}: invalid consumer ownership: {consumer:?}"
            ),
            Self::DuplicateDependency {
                contribution,
                dependency,
            } => write!(
                f,
                "duplicate dependency for {contribution:?}: {dependency:?}"
            ),
            Self::MissingDependency {
                contribution,
                dependency,
            } => write!(f, "missing dependency for {contribution:?}: {dependency:?}"),
            Self::RoutesWithoutProducer { contribution } => {
                write!(f, "routes declared without producer: {contribution:?}")
            }
            Self::UnknownRoute {
                contribution,
                route,
            } => write!(f, "unknown producer route for {contribution:?}: {route:?}"),
            Self::DuplicateRoute {
                contribution,
                route,
            } => write!(
                f,
                "duplicate producer route for {contribution:?}: {route:?}"
            ),
        }
    }
}
impl std::error::Error for ConfigurationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateError {
    ActivationAlreadyRegistered,
    ExportAlreadyRegistered,
    UndeclaredImport { id: String },
    InspectionNotGranted,
}
impl fmt::Display for CandidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActivationAlreadyRegistered => {
                f.write_str("candidate activation already registered")
            }
            Self::ExportAlreadyRegistered => f.write_str("candidate export already registered"),
            Self::UndeclaredImport { id } => write!(f, "undeclared import: {id}"),
            Self::InspectionNotGranted => {
                f.write_str("full compiled network inspection not granted")
            }
        }
    }
}
impl std::error::Error for CandidateError {}
