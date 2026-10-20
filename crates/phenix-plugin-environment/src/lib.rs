//! Default environment provider. Resolves locations without filesystem I/O.
//! The selected workspace, configuration and storage providers own all effects.
pub use phenix_environment_contracts::local::{resolve_paths, DirectoryInputs};

#[phenix_sdk::plugin(
    id = "phenix.environment.default",
    version = "1.0.0",
    profile = "state",
    provide(phenix_environment_contracts::Environment, slot = "Environment")
)]
mod plugin {
    use super::{resolve_paths, DirectoryInputs};
    use phenix_environment_contracts::{
        Environment, EnvironmentError, EnvironmentPaths, EnvironmentProblem, EnvironmentQuery,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    fn error(problem: EnvironmentProblem, message: &str) -> EnvironmentError {
        EnvironmentError {
            problem,
            message: message.into(),
        }
    }

    pub struct State {
        inputs: DirectoryInputs,
    }

    impl Default for State {
        fn default() -> Self {
            Self {
                inputs: DirectoryInputs::capture(),
            }
        }
    }

    #[provide(Environment::resolve)]
    fn resolve(
        state: &mut State,
        host: &Host<'_>,
        request: EnvironmentQuery,
    ) -> CallResult<EnvironmentPaths, EnvironmentError> {
        if !phenix_sdk::identifier(&request.resource) {
            return Err(CallError::Domain(error(
                EnvironmentProblem::InvalidInput,
                "invalid environment resource",
            )));
        }
        if host.resource()?.as_deref() != Some(request.resource.as_str()) {
            return Err(CallError::Domain(error(
                EnvironmentProblem::Denied,
                "environment resource differs from authorized root",
            )));
        }
        resolve_paths(&state.inputs, &request).map_err(CallError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_environment_contracts::{EnvironmentProblem, EnvironmentQuery};

    fn request() -> EnvironmentQuery {
        EnvironmentQuery {
            resource: "phenix.session.s1".into(),
            namespace: "example.plugin".into(),
        }
    }

    #[test]
    fn xdg_and_workspace_are_resolved_independently() {
        let inputs = DirectoryInputs {
            home: Some("/home/user".into()),
            cwd: Some("/work/repo".into()),
            config: Some("/custom/config".into()),
            state: Some("/custom/state".into()),
            data: Some("/custom/data".into()),
            cache: Some("/custom/cache".into()),
            runtime: Some("/run/user/1000".into()),
        };
        let result = resolve_paths(&inputs, &request()).unwrap();
        assert_eq!(result.launch_cwd.as_deref(), Some("/work/repo"));
        assert_eq!(result.config, "/custom/config/phenix/example.plugin");
        assert_eq!(result.state, "/custom/state/phenix/example.plugin");
        assert_eq!(result.data, "/custom/data/phenix/example.plugin");
        assert_eq!(result.cache, "/custom/cache/phenix/example.plugin");
        assert_eq!(
            result.runtime.as_deref(),
            Some("/run/user/1000/phenix/example.plugin")
        );
    }

    #[test]
    fn relative_xdg_is_ignored_and_runtime_is_optional() {
        let inputs = DirectoryInputs {
            home: Some("/home/user".into()),
            cwd: Some("relative".into()),
            config: Some("relative/config".into()),
            state: Some("/abs/../unsafe".into()),
            runtime: Some("relative/runtime".into()),
            ..DirectoryInputs::default()
        };
        let result = resolve_paths(&inputs, &request()).unwrap();
        assert_eq!(result.config, "/home/user/.config/phenix/example.plugin");
        assert_eq!(
            result.state,
            "/home/user/.local/state/phenix/example.plugin"
        );
        assert_eq!(result.data, "/home/user/.local/share/phenix/example.plugin");
        assert_eq!(result.cache, "/home/user/.cache/phenix/example.plugin");
        assert_eq!(result.runtime, None);
        assert_eq!(result.launch_cwd, None);
    }

    #[test]
    fn no_home_or_xdg_fails_closed() {
        let error = resolve_paths(&DirectoryInputs::default(), &request()).unwrap_err();
        assert_eq!(error.problem, EnvironmentProblem::Unavailable);
    }

    #[test]
    fn namespace_cannot_escape_plugin_directory() {
        let inputs = DirectoryInputs {
            home: Some("/home/user".into()),
            ..DirectoryInputs::default()
        };
        for invalid in ["../outside", "..", ".", "plugin/../../outside", ""] {
            let mut request = request();
            request.namespace = invalid.into();
            assert_eq!(
                resolve_paths(&inputs, &request).unwrap_err().problem,
                EnvironmentProblem::InvalidInput
            );
        }
    }
}
