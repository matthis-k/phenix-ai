#![forbid(unsafe_code)]

#[cfg(test)]
use phenix_client::ServiceOutput;
use phenix_client::{ServiceRequest, ServiceResponse};
use phenix_core::{
    Authority, GraphGenerationId, Kernel, KernelError, PluginManifest, ResolvedHarness,
    ResolvedHarnessActivation, ResolvedHarnessActivationError, ResolvedHarnessError, ServiceId,
};
use serde_json::Value;
use std::{
    fmt,
    io::{self, BufRead, Write},
};

/// Generic configured Phenix server runtime.
///
/// Product plugin selection belongs to the Harness. A conductor created with no
/// manifests therefore exposes no first-party services.
pub struct Conductor {
    kernel: Kernel,
    resolved: ResolvedHarness,
}

#[derive(Debug)]
pub enum ConductorBuildError {
    Resolution(ResolvedHarnessError),
    Activation(ResolvedHarnessActivationError),
}

impl fmt::Display for ConductorBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolution(error) => fmt::Display::fmt(error, formatter),
            Self::Activation(error) => {
                write!(formatter, "resolved conductor activation failed: {error:?}")
            }
        }
    }
}

impl std::error::Error for ConductorBuildError {}

impl From<ResolvedHarnessError> for ConductorBuildError {
    fn from(error: ResolvedHarnessError) -> Self {
        Self::Resolution(error)
    }
}

impl From<ResolvedHarnessActivationError> for ConductorBuildError {
    fn from(error: ResolvedHarnessActivationError) -> Self {
        Self::Activation(error)
    }
}

impl Conductor {
    pub fn new(
        manifests: impl IntoIterator<Item = PluginManifest>,
    ) -> Result<Self, ConductorBuildError> {
        let resolved = ResolvedHarness::resolve(manifests, [], [], &Authority::default())?;
        let mut kernel = Kernel::new(resolved.kernel_config().clone());
        kernel.activate_resolved_harness(&resolved)?;
        Ok(Self { kernel, resolved })
    }

    #[must_use]
    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    pub fn kernel_mut(&mut self) -> &mut Kernel {
        &mut self.kernel
    }

    #[must_use]
    pub fn resolved_harness(&self) -> &ResolvedHarness {
        &self.resolved
    }

    #[must_use]
    pub fn generation(&self) -> &GraphGenerationId {
        self.resolved.generation()
    }

    pub fn activate_all(&mut self) -> Result<(), KernelError> {
        self.kernel.activate_all()
    }

    pub fn serve_jsonl<R: BufRead, W: Write>(
        &mut self,
        authority: &Authority,
        reader: R,
        writer: W,
    ) -> Result<(), ServeError> {
        serve_jsonl(&mut self.kernel, authority, reader, writer)
    }
}

impl Default for Conductor {
    fn default() -> Self {
        let resolved = ResolvedHarness::resolve([], [], [], &Authority::default())
            .expect("empty conductor composition is valid");
        let mut kernel = Kernel::new(resolved.kernel_config().clone());
        kernel
            .activate_resolved_harness(&resolved)
            .expect("empty resolved conductor composition activates");
        Self { kernel, resolved }
    }
}

#[derive(Debug)]
pub enum ServeError {
    Io(io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for ServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "service transport I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "service response encoding failed: {error}"),
        }
    }
}

impl std::error::Error for ServeError {}

impl From<io::Error> for ServeError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ServeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[must_use]
pub fn handle_service_request(
    kernel: &mut Kernel,
    authority: &Authority,
    line: &str,
) -> ServiceResponse {
    let request = match serde_json::from_str::<ServiceRequest>(line) {
        Ok(request) => request,
        Err(error) => return ServiceResponse::error(Value::Null, error.to_string()),
    };
    let id = request.id;
    let service = match ServiceId::parse(request.service) {
        Ok(service) => service,
        Err(error) => return ServiceResponse::error(id, error.to_string()),
    };
    let input = match serde_json::to_vec(&request.input) {
        Ok(input) => input,
        Err(error) => return ServiceResponse::error(id, error.to_string()),
    };

    match kernel.invoke(&service, &input, authority, None) {
        Ok(output) => match serde_json::from_slice::<Value>(&output) {
            Ok(output) => ServiceResponse::json(id, output),
            Err(_) => ServiceResponse::bytes(id, output),
        },
        Err(error) => ServiceResponse::error(id, error.to_string()),
    }
}

pub fn serve_jsonl<R: BufRead, W: Write>(
    kernel: &mut Kernel,
    authority: &Authority,
    reader: R,
    mut writer: W,
) -> Result<(), ServeError> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_service_request(kernel, authority, &line);
        serde_json::to_writer(&mut writer, &response)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        PluginExecution, PluginHost, PluginId, PluginInstance, ResourceNamespace,
        ServiceContribution,
    };

    fn fixture_manifest(plugin: &str) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(plugin).unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: ServiceId::parse("fixture.echo@1").unwrap(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::<ResourceNamespace>::new(),
            maximum_authority: Authority::default(),
        }
    }

    struct Echo(&'static [u8]);

    impl PluginInstance for Echo {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            _service: &ServiceId,
            _input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            Ok(self.0.to_vec())
        }
    }

    fn configured_fixture(plugin: &str, output: &'static [u8]) -> Conductor {
        let manifest = fixture_manifest(plugin);
        let plugin = manifest.id.clone();
        let mut conductor = Conductor::new([manifest]).unwrap();
        conductor
            .kernel_mut()
            .register_embedded_factory(plugin, move || Box::new(Echo(output)))
            .unwrap();
        conductor.activate_all().unwrap();
        conductor
    }

    fn invoke_fixture(conductor: &mut Conductor) -> ServiceResponse {
        handle_service_request(
            conductor.kernel_mut(),
            &Authority::default(),
            r#"{"id":2,"service":"fixture.echo@1","input":{}}"#,
        )
    }

    #[test]
    fn zero_plugin_conductor_has_no_first_party_fallback() {
        let mut conductor = Conductor::default();
        conductor.activate_all().unwrap();
        assert_eq!(conductor.kernel().config().manifests().count(), 0);
        assert_eq!(
            conductor.kernel().graph_generation(),
            Some(conductor.generation())
        );
    }

    #[test]
    fn zero_plugin_transport_reports_missing_service() {
        let mut conductor = Conductor::default();
        conductor.activate_all().unwrap();
        let input = b"{\"id\":1,\"service\":\"phenix.sessions@1\",\"input\":{}}\n";
        let mut output = Vec::new();
        conductor
            .serve_jsonl(&Authority::default(), &input[..], &mut output)
            .unwrap();
        let response: ServiceResponse = serde_json::from_slice(&output).unwrap();
        assert!(matches!(
            response,
            ServiceResponse::Error { id, .. } if id == serde_json::json!(1)
        ));
    }

    #[test]
    fn conductor_runs_exactly_one_configured_plugin() {
        let mut conductor = configured_fixture("fixture.primary", br#"{"provider":"primary"}"#);
        assert_eq!(conductor.kernel().config().manifests().count(), 1);
        assert_eq!(
            conductor.kernel().graph_generation(),
            Some(conductor.generation())
        );
        assert!(matches!(
            invoke_fixture(&mut conductor),
            ServiceResponse::Ok {
                output: ServiceOutput::Json { output },
                ..
            } if output == serde_json::json!({"provider": "primary"})
        ));
    }

    #[test]
    fn replacement_plugin_uses_the_same_conductor_service_contract() {
        let mut primary = configured_fixture("fixture.primary", br#"{"provider":"primary"}"#);
        let mut replacement =
            configured_fixture("fixture.replacement", br#"{"provider":"replacement"}"#);

        assert!(matches!(
            invoke_fixture(&mut primary),
            ServiceResponse::Ok {
                output: ServiceOutput::Json { output },
                ..
            } if output == serde_json::json!({"provider": "primary"})
        ));
        assert!(matches!(
            invoke_fixture(&mut replacement),
            ServiceResponse::Ok {
                output: ServiceOutput::Json { output },
                ..
            } if output == serde_json::json!({"provider": "replacement"})
        ));
    }
}
