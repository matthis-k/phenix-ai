//! Default environment provider. Resolves locations without filesystem I/O.
//! The selected workspace, configuration and storage providers own all effects.
use phenix_environment_contracts::{
    EnvironmentError, EnvironmentPaths, EnvironmentProblem, EnvironmentQuery,
};
use std::path::{Component, Path, PathBuf};

fn error(problem: EnvironmentProblem, message: &str) -> EnvironmentError {
    EnvironmentError {
        problem,
        message: message.into(),
    }
}

fn usable(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

fn path(value: Option<&PathBuf>) -> Option<&Path> {
    value.map(PathBuf::as_path).filter(|path| usable(path))
}

fn as_utf8(path: &Path) -> Result<String, EnvironmentError> {
    path.to_str().map(str::to_string).ok_or_else(|| {
        error(
            EnvironmentProblem::Unavailable,
            "environment path is not UTF-8",
        )
    })
}

/// A captured launch environment is immutable for the lifetime of the plugin.
/// Injecting it here also permits tests without changing process environment.
#[derive(Debug, Clone, Default)]
pub struct DirectoryInputs {
    pub home: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub state: Option<PathBuf>,
    pub data: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
}

impl DirectoryInputs {
    pub fn capture() -> Self {
        Self {
            home: std::env::var_os("HOME").map(PathBuf::from),
            cwd: std::env::current_dir().ok(),
            config: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            state: std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
            data: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            cache: std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            runtime: std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        }
    }
}

fn base(
    override_path: Option<&PathBuf>,
    home: Option<&Path>,
    fallback: &str,
    namespace: &str,
) -> Result<String, EnvironmentError> {
    let root = path(override_path)
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(fallback)))
        .ok_or_else(|| {
            error(
                EnvironmentProblem::Unavailable,
                "no absolute XDG directory or HOME is configured",
            )
        })?;
    as_utf8(&root.join("phenix").join(namespace))
}

/// Resolve XDG defaults without creating directories, opening files or
/// treating a returned path as permission to read or write.
pub fn resolve_paths(
    inputs: &DirectoryInputs,
    request: &EnvironmentQuery,
) -> Result<EnvironmentPaths, EnvironmentError> {
    let namespace = request.namespace.as_str();
    if namespace.len() > 128
        || !phenix_sdk::identifier(namespace)
        || namespace == "."
        || namespace == ".."
    {
        return Err(error(
            EnvironmentProblem::InvalidInput,
            "invalid environment namespace",
        ));
    }
    let home = path(inputs.home.as_ref());
    let runtime = path(inputs.runtime.as_ref())
        .map(|runtime| as_utf8(&runtime.join("phenix").join(namespace)))
        .transpose()?;
    let workspace = path(inputs.cwd.as_ref()).map(as_utf8).transpose()?;
    Ok(EnvironmentPaths {
        resource: request.resource.clone(),
        workspace,
        config: base(inputs.config.as_ref(), home, ".config", namespace)?,
        state: base(inputs.state.as_ref(), home, ".local/state", namespace)?,
        data: base(inputs.data.as_ref(), home, ".local/share", namespace)?,
        cache: base(inputs.cache.as_ref(), home, ".cache", namespace)?,
        runtime,
    })
}

#[phenix_sdk::plugin(
    id = "phenix.environment.default",
    version = "1.0.0",
    profile = "state",
    provide(phenix_environment_contracts::Environment, slot = "Environment")
)]
mod plugin {
    use super::{error, resolve_paths, DirectoryInputs};
    use phenix_environment_contracts::{
        Environment, EnvironmentError, EnvironmentPaths, EnvironmentProblem, EnvironmentQuery,
    };
    use phenix_sdk::{CallError, CallResult, Host};

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
        assert_eq!(result.workspace.as_deref(), Some("/work/repo"));
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
        assert_eq!(result.workspace, None);
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
