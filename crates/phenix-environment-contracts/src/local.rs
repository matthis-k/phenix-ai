//! Pure directory policy shared by the environment plugin and product hosts.
//! No host calls, process mutation, filesystem creation or grant checks.
use super::{EnvironmentError, EnvironmentPaths, EnvironmentProblem, EnvironmentQuery};
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
    let launch_cwd = path(inputs.cwd.as_ref()).map(as_utf8).transpose()?;
    Ok(EnvironmentPaths {
        resource: request.resource.clone(),
        launch_cwd,
        config: base(inputs.config.as_ref(), home, ".config", namespace)?,
        state: base(inputs.state.as_ref(), home, ".local/state", namespace)?,
        data: base(inputs.data.as_ref(), home, ".local/share", namespace)?,
        cache: base(inputs.cache.as_ref(), home, ".cache", namespace)?,
        runtime,
    })
}

/// Shared Basic-state root policy for product hosts and providers. It returns
/// the OS-specific base, before the application namespace and file name.
pub fn state_root(inputs: &DirectoryInputs) -> Result<PathBuf, EnvironmentError> {
    let home = path(inputs.home.as_ref());
    path(inputs.state.as_ref())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".local/state")))
        .ok_or_else(|| {
            error(
                EnvironmentProblem::Unavailable,
                "no absolute XDG state directory or HOME is configured",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_root_matches_xdg_and_home_fallback() {
        let home = Some(PathBuf::from("/home/user"));
        let mut inputs = DirectoryInputs {
            home: home.clone(),
            state: Some("/custom/state".into()),
            ..Default::default()
        };
        assert_eq!(state_root(&inputs).unwrap(), PathBuf::from("/custom/state"));
        for invalid in [
            None,
            Some("".into()),
            Some("relative".into()),
            Some("/tmp/../bad".into()),
        ] {
            inputs.state = invalid;
            assert_eq!(
                state_root(&inputs).unwrap(),
                PathBuf::from("/home/user/.local/state")
            );
        }
        inputs.home = None;
        assert!(state_root(&inputs).is_err());
        inputs.state = Some("/abs/state".into());
        assert_eq!(state_root(&inputs).unwrap(), PathBuf::from("/abs/state"));
    }
}
