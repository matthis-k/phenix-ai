#![forbid(unsafe_code)]

mod runtime;
mod store;

pub use store::{CredentialStore, CredentialStoreError};

use phenix_core::PluginInstance;
use phenix_provider_sdk::{ProviderDefinition, ProviderError};
use std::sync::Arc;

pub const PHENIX_CA_BUNDLE_ENV: &str = "PHENIX_CA_BUNDLE";

pub fn provider_factory(
    definition: &ProviderDefinition,
) -> impl Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static {
    let spec = definition.runtime_spec();
    move || Box::new(runtime::ProviderPlugin::new(Arc::clone(&spec)))
}

pub fn provider_http_client_builder() -> Result<reqwest::ClientBuilder, ProviderError> {
    let mut builder = reqwest::Client::builder().tls_backend_rustls();
    let Some(path) =
        provider_ca_bundle_from(|name| std::env::var_os(name).map(std::path::PathBuf::from))
    else {
        return Ok(builder);
    };
    let source = PHENIX_CA_BUNDLE_ENV;
    let pem = std::fs::read(&path).map_err(|error| ProviderError::Transport {
        message: format!(
            "cannot read CA bundle from {source} ({}): {error}",
            path.display()
        ),
    })?;
    let certificates =
        reqwest::Certificate::from_pem_bundle(&pem).map_err(|error| ProviderError::Transport {
            message: format!(
                "cannot parse CA bundle from {source} ({}): {error}",
                path.display()
            ),
        })?;
    if certificates.is_empty() {
        return Err(ProviderError::Transport {
            message: format!(
                "CA bundle from {source} ({}) contains no certificates",
                path.display()
            ),
        });
    }
    builder = builder.tls_certs_only(certificates);
    Ok(builder)
}

fn provider_ca_bundle_from(
    mut value: impl FnMut(&str) -> Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    value(PHENIX_CA_BUNDLE_ENV)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        model_inference_service, Authority, Kernel, KernelConfig, ModelInferenceRequest,
        ModelInferenceResponse, ModelId, Project,
    };
    use phenix_provider_sdk::{auth, Endpoint, Protocol};
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn ca_bundle_selection_uses_only_the_explicit_phenix_override() {
        let explicit = provider_ca_bundle_from(|name| {
            (name == PHENIX_CA_BUNDLE_ENV)
                .then(|| std::path::PathBuf::from("/missing/explicit.pem"))
        });
        assert_eq!(
            explicit,
            Some(std::path::PathBuf::from("/missing/explicit.pem"))
        );

        let inherited = provider_ca_bundle_from(|name| match name {
            "SSL_CERT_FILE" => Some(std::path::PathBuf::from("/missing/ssl.pem")),
            "NIX_SSL_CERT_FILE" => Some(std::path::PathBuf::from("/nix/store/ca-bundle.crt")),
            _ => None,
        });
        assert_eq!(inherited, None);
    }

    #[test]
    fn generated_provider_executes_protocol_end_to_end() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("POST /v1/responses HTTP/1.1"));
            assert!(request.contains("\"model\":\"model-a\""));
            assert!(request.contains("\"input\":\"hello\""));

            let body = r#"{"id":"response-1","output":[{"content":[{"type":"output_text","text":"world"}]}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-ratelimit-limit-requests: 100\r\nx-ratelimit-remaining-requests: 99\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let definition = ProviderDefinition::new(
            phenix_core::PluginId::parse("provider.local").unwrap(),
            Endpoint::parse(format!("http://{address}/v1")).unwrap(),
            Protocol::OpenAiResponses,
            auth::Definition::none(),
        );
        let manifest = definition.manifest();
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        kernel
            .register_embedded_factory(plugin.clone(), provider_factory(&definition))
            .unwrap();
        kernel.activate_all().unwrap();

        let output = kernel
            .invoke(
                &model_inference_service(),
                &serde_json::to_vec(&phenix_core::PhenixValue::from(&ModelInferenceRequest {
                    session_id: None,
                    model: ModelId::parse("model-a").unwrap(),
                    input: b"hello".to_vec().into(),
                    options: BTreeMap::new(),
                    cache: Default::default(),
                    tools: Vec::new(),
                    continuation: Vec::new(),
                }))
                .unwrap(),
                &Authority::new([phenix_core::CapabilityId::parse("network.http").unwrap()]),
                Some(&plugin),
            )
            .unwrap();
        let output: phenix_core::PhenixValue = serde_json::from_slice(&output).unwrap();
        let response = ModelInferenceResponse::try_from(Project(&output)).unwrap();
        assert_eq!(response.output.as_ref(), b"world");
        server.join().unwrap();
    }
}
