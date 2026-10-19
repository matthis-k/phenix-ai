//! Typed locations for an authorized runtime resource.
//!
//! Paths are hints for selected filesystem/configuration providers, not
//! capabilities, filesystem handles or permission to access those paths.
use phenix_sdk::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EnvironmentQuery {
    /// Exact caller resource, checked against the active kernel grant.
    pub resource: String,
    /// One filesystem-safe plugin namespace, not an arbitrary path.
    pub namespace: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EnvironmentPaths {
    pub resource: String,
    pub workspace: Option<String>,
    pub config: String,
    pub state: String,
    pub data: String,
    pub cache: String,
    pub runtime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum EnvironmentProblem {
    InvalidInput,
    Denied,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EnvironmentError {
    pub problem: EnvironmentProblem,
    pub message: String,
}

#[contract(id = "phenix.environment@1")]
pub trait Environment {
    fn resolve(&self, request: EnvironmentQuery) -> Result<EnvironmentPaths, EnvironmentError>;
}

#[plugin(
    id = "phenix.environment-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(Environment)
)]
pub mod definitions {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_snapshot_are_portable() {
        let query = EnvironmentQuery {
            resource: "phenix.session.s1".into(),
            namespace: "phenix.basic.memory".into(),
        };
        assert_eq!(
            EnvironmentQuery::from_value(query.to_value()).unwrap(),
            query
        );
        let paths = EnvironmentPaths {
            resource: query.resource,
            workspace: None,
            config: "/tmp/config".into(),
            state: "/tmp/state".into(),
            data: "/tmp/data".into(),
            cache: "/tmp/cache".into(),
            runtime: None,
        };
        assert_eq!(
            EnvironmentPaths::from_value(paths.to_value()).unwrap(),
            paths
        );
    }
}
