use crate::{
    ContextInterface, ModelRoutingInterface, OptionsInterface, SdkConfigInterface,
    SdkSessionCommand, SdkSessionInterface, SdkSessionResponse, SdkSkillsInterface,
    SdkToolsInterface, SessionCommand, SessionId, SessionInterface, SessionRecord, SessionResponse,
};
#[cfg(test)]
use phenix_core::Authority;
pub use phenix_core::{
    CallContext, CurrentPlugin, KernelAccess, PluginContext, SdkClient, SdkContract, SdkObject,
};
use phenix_core::{ComponentId, ComponentInterface, PluginHost};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

pub type PhenixPluginContext<'host, 'runtime, Settings = (), State = ()> =
    PluginContext<'host, 'runtime, PhenixSdk<'host, 'runtime>, Settings, State>;

pub fn phenix_context<'host, 'runtime, Settings, State>(
    host: &'host PluginHost<'runtime>,
    component: ComponentId,
    settings: Settings,
    state: State,
) -> PhenixPluginContext<'host, 'runtime, Settings, State> {
    PluginContext::new(host, PhenixSdk::new(host, component), settings, state)
}

struct SdkAccess<'host, 'runtime> {
    host: &'host PluginHost<'runtime>,
    component: ComponentId,
}

impl<'host, 'runtime> SdkAccess<'host, 'runtime> {
    fn new(host: &'host PluginHost<'runtime>, component: ComponentId) -> Self {
        Self { host, component }
    }

    fn client<I: ComponentInterface>(&self) -> SdkClient<'host, 'runtime, I> {
        SdkClient::new(self.host, self.component.clone())
    }
}

/// Default Phenix userspace SDK available to a plugin.
pub struct PhenixSdk<'host, 'runtime> {
    pub sessions: Sessions<'host, 'runtime>,
    pub models: SdkClient<'host, 'runtime, ModelRoutingInterface>,
    pub tools: SdkClient<'host, 'runtime, SdkToolsInterface>,
    pub skills: SdkClient<'host, 'runtime, SdkSkillsInterface>,
    pub context: SdkClient<'host, 'runtime, ContextInterface>,
    pub options: SdkClient<'host, 'runtime, OptionsInterface>,
    pub config: SdkClient<'host, 'runtime, SdkConfigInterface>,
    extensions: SdkAccess<'host, 'runtime>,
}

impl<'host, 'runtime> PhenixSdk<'host, 'runtime> {
    pub fn new(host: &'host PluginHost<'runtime>, component: ComponentId) -> Self {
        let access = SdkAccess::new(host, component);
        Self {
            sessions: Sessions::new(
                access.client::<SdkSessionInterface>(),
                access.client::<SessionInterface>(),
            ),
            models: access.client::<ModelRoutingInterface>(),
            tools: access.client::<SdkToolsInterface>(),
            skills: access.client::<SdkSkillsInterface>(),
            context: access.client::<ContextInterface>(),
            options: access.client::<OptionsInterface>(),
            config: access.client::<SdkConfigInterface>(),
            extensions: access,
        }
    }

    /// Bind an SDK contract declared by this plugin.
    ///
    /// Invocation still goes through the caller component import, so undeclared
    /// or unbound dependencies fail at the kernel boundary.
    pub fn require<C: SdkContract>(&self) -> SdkClient<'host, 'runtime, C::Interface> {
        self.extensions.client::<C::Interface>()
    }
}

#[derive(Debug)]
pub enum SdkError {
    Invocation(phenix_core::ComponentInvocationError),
    InvalidIdentifier {
        kind: &'static str,
        message: &'static str,
    },
    UnexpectedResponse {
        operation: &'static str,
    },
}

impl Display for SdkError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invocation(error) => Display::fmt(error, f),
            Self::InvalidIdentifier { kind, message } => {
                write!(f, "invalid {kind} identifier: {message}")
            }
            Self::UnexpectedResponse { operation } => {
                write!(f, "unexpected SDK response while {operation}")
            }
        }
    }
}

impl Error for SdkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invocation(error) => Some(error),
            Self::InvalidIdentifier { .. } | Self::UnexpectedResponse { .. } => None,
        }
    }
}

impl From<phenix_core::ComponentInvocationError> for SdkError {
    fn from(error: phenix_core::ComponentInvocationError) -> Self {
        Self::Invocation(error)
    }
}

#[derive(Clone)]
pub struct Sessions<'host, 'runtime> {
    policy: SdkClient<'host, 'runtime, SdkSessionInterface>,
    storage: SdkClient<'host, 'runtime, SessionInterface>,
}

impl<'host, 'runtime> Sessions<'host, 'runtime> {
    fn new(
        policy: SdkClient<'host, 'runtime, SdkSessionInterface>,
        storage: SdkClient<'host, 'runtime, SessionInterface>,
    ) -> Self {
        Self { policy, storage }
    }

    pub fn open(&self, id: impl Into<String>) -> Result<Session<'host, 'runtime>, SdkError> {
        Ok(self.open_with_status(id, None)?.session)
    }

    pub fn open_for_agent(
        &self,
        id: impl Into<String>,
        agent: impl Into<String>,
    ) -> Result<Session<'host, 'runtime>, SdkError> {
        Ok(self.open_with_status(id, Some(agent.into()))?.session)
    }

    pub fn open_with_status(
        &self,
        id: impl Into<String>,
        agent: Option<String>,
    ) -> Result<OpenedSession<'host, 'runtime>, SdkError> {
        let response = self
            .policy
            .invoke::<SdkSessionCommand, SdkSessionResponse>(&SdkSessionCommand::Open {
                id: id.into(),
                agent,
            })?;
        let SdkSessionResponse::Opened { session, created } = response;
        Ok(OpenedSession {
            session: Session::new(session, self.clone()),
            created,
        })
    }

    pub fn find(
        &self,
        id: impl Into<String>,
    ) -> Result<Option<Session<'host, 'runtime>>, SdkError> {
        let id = SessionId::parse(id.into()).map_err(|message| SdkError::InvalidIdentifier {
            kind: "session",
            message,
        })?;
        let response = self.storage.invoke(&SessionCommand::Get { id })?;
        let SessionResponse::Session { session } = response else {
            return Err(SdkError::UnexpectedResponse {
                operation: "finding session",
            });
        };
        Ok(session.map(|session| Session::new(session, self.clone())))
    }

    pub fn iter(&self) -> Result<impl Iterator<Item = Session<'host, 'runtime>>, SdkError> {
        let response = self.storage.invoke(&SessionCommand::List)?;
        let SessionResponse::Sessions { sessions: records } = response else {
            return Err(SdkError::UnexpectedResponse {
                operation: "listing sessions",
            });
        };
        let sessions = self.clone();
        Ok(records
            .into_iter()
            .map(move |record| Session::new(record, sessions.clone())))
    }
}

pub struct OpenedSession<'host, 'runtime> {
    pub session: Session<'host, 'runtime>,
    pub created: bool,
}

#[derive(Clone)]
pub struct Session<'host, 'runtime> {
    record: SessionRecord,
    sessions: Sessions<'host, 'runtime>,
}

impl<'host, 'runtime> Session<'host, 'runtime> {
    fn new(record: SessionRecord, sessions: Sessions<'host, 'runtime>) -> Self {
        Self { record, sessions }
    }

    pub fn id(&self) -> &str {
        self.record.id.as_str()
    }

    pub fn record(&self) -> &SessionRecord {
        &self.record
    }

    pub fn into_record(self) -> SessionRecord {
        self.record
    }

    pub fn refresh(&self) -> Result<Option<Self>, SdkError> {
        self.sessions.find(self.id().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sdk_component_manifest, sdk_factory, sdk_manifest};
    use phenix_core::{
        CapabilityId, ComponentExport, ComponentImport, ComponentManifest, InterfaceId, Kernel,
        KernelConfig, PluginExecution, PluginId, PluginInstance, PluginManifest, ResolvedHarness,
        ResolvedHarnessActivation, ServiceContribution, ServiceId, ServiceRole,
    };
    use phenix_plugin_options_test::{
        options_component_manifest, options_factory, options_manifest,
    };
    use phenix_plugin_sessions_test::{
        session_component_manifest, session_factory, session_manifest,
    };
    use phenix_sdk_macros::PhenixValue;
    use serde::{Deserialize, Serialize};

    const ECHO_PLUGIN: &str = "fixture.echo";
    const ECHO_COMPONENT: &str = "fixture.echo";
    const ECHO_SERVICE: &str = "fixture.echo@1";
    const CONSUMER_PLUGIN: &str = "fixture.consumer";
    const CONSUMER_COMPONENT: &str = "fixture.consumer";
    const CONSUMER_SERVICE: &str = "fixture.consumer.run@1";

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize, PhenixValue)]
    struct EchoRequest {
        value: String,
    }

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize, PhenixValue)]
    struct EchoResponse {
        value: String,
        has_persistence_authority: bool,
    }

    struct EchoInterface;

    impl ComponentInterface for EchoInterface {
        fn interface_id() -> InterfaceId {
            InterfaceId::parse(ECHO_SERVICE).unwrap()
        }

        fn schema() -> phenix_core::InterfaceSchema {
            phenix_core::InterfaceSchema::of::<EchoRequest, EchoResponse>()
        }
    }

    struct EchoSdk;

    impl SdkContract for EchoSdk {
        type Interface = EchoInterface;
    }

    struct EchoPlugin;

    impl PluginInstance for EchoPlugin {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &echo_service() {
                return Err(format!("unsupported echo service: {service}"));
            }
            let context: PluginContext<'_, '_, ()> = PluginContext::new(host, (), (), ());
            let request = context
                .kernel
                .decode::<EchoRequest>(&EchoInterface::interface_id(), input)
                .map_err(|error| error.to_string())?;
            context
                .kernel
                .encode_value(&EchoResponse {
                    value: request.value,
                    has_persistence_authority: host
                        .authority()
                        .permits(&CapabilityId::parse("kernel.persistence.read").unwrap()),
                })
                .map_err(|error| error.to_string())
        }
    }

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct ConsumerOutput {
        plugin: String,
        settings: String,
        state: u32,
        session: String,
        echo: String,
        echo_has_persistence_authority: bool,
        has_persistence_authority: bool,
    }

    struct ConsumerPlugin;

    impl PluginInstance for ConsumerPlugin {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            _input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &consumer_service() {
                return Err(format!("unsupported consumer service: {service}"));
            }

            let context = phenix_context(
                host,
                consumer_component_id(),
                "configured".to_owned(),
                7_u32,
            );
            let PluginContext {
                sdk, plugin, call, ..
            } = context;

            let opened = sdk
                .sessions
                .open("root")
                .map_err(|error| error.to_string())?;
            let found = sdk
                .sessions
                .iter()
                .map_err(|error| error.to_string())?
                .find(|session| session.id() == opened.id())
                .ok_or("session missing from SDK iterator")?;
            let refreshed = found
                .refresh()
                .map_err(|error| error.to_string())?
                .ok_or("session disappeared while refreshing SDK object")?;

            let echo = SdkObject::new("echo", sdk.require::<EchoSdk>());
            let echo: EchoResponse = echo
                .client()
                .invoke(&EchoRequest {
                    value: refreshed.id().to_owned(),
                })
                .map_err(|error| error.to_string())?;

            serde_json::to_vec(&ConsumerOutput {
                plugin: plugin.id.as_str().to_owned(),
                settings: plugin.settings,
                state: plugin.state,
                session: refreshed.id().to_owned(),
                echo: echo.value,
                echo_has_persistence_authority: echo.has_persistence_authority,
                has_persistence_authority: call
                    .authority
                    .permits(&CapabilityId::parse("kernel.persistence.read").unwrap()),
            })
            .map_err(|error| error.to_string())
        }
    }

    fn authority() -> Authority {
        Authority::new([
            CapabilityId::parse("kernel.persistence.schema").unwrap(),
            CapabilityId::parse("kernel.persistence.read").unwrap(),
            CapabilityId::parse("kernel.persistence.write").unwrap(),
        ])
    }

    fn echo_service() -> ServiceId {
        ServiceId::parse(ECHO_SERVICE).unwrap()
    }

    fn consumer_service() -> ServiceId {
        ServiceId::parse(CONSUMER_SERVICE).unwrap()
    }

    fn consumer_component_id() -> ComponentId {
        ComponentId::parse(CONSUMER_COMPONENT).unwrap()
    }

    fn echo_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(ECHO_PLUGIN).unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service: echo_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn echo_component_manifest() -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse(ECHO_COMPONENT).unwrap(),
            owner: PluginId::parse(ECHO_PLUGIN).unwrap(),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: EchoInterface::interface_id(),
                schema: EchoInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        }
    }

    fn consumer_manifest(authority: Authority) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(CONSUMER_PLUGIN).unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service: consumer_service(),
                priority: 100,
                required_authority: authority.clone(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: authority,
        }
    }

    fn consumer_component_manifest(authority: Authority) -> ComponentManifest {
        let required = |interface, schema| ComponentImport {
            interface,
            schema,
            required: true,
            authority: authority.clone(),
        };
        ComponentManifest {
            listeners: Vec::new(),
            id: consumer_component_id(),
            owner: PluginId::parse(CONSUMER_PLUGIN).unwrap(),
            imports: vec![
                required(
                    SdkSessionInterface::interface_id(),
                    SdkSessionInterface::schema(),
                ),
                required(SessionInterface::interface_id(), SessionInterface::schema()),
                required(EchoInterface::interface_id(), EchoInterface::schema()),
            ],
            exports: Vec::new(),
            maximum_authority: authority,
        }
    }

    #[test]
    fn context_sdk_and_provider_owned_objects_remain_kernel_mediated() {
        let authority = authority();
        let session = session_manifest();
        let options = options_manifest();
        let sdk = sdk_manifest(authority.clone());
        let echo = echo_manifest();
        let consumer = consumer_manifest(authority.clone());
        let manifests = vec![
            session.clone(),
            options.clone(),
            sdk.clone(),
            echo.clone(),
            consumer.clone(),
        ];
        let durable_schemas = phenix_plugin_options_test::options_durable_schema_registrations();
        let resolved = ResolvedHarness::resolve_with_durable_schemas(
            manifests.clone(),
            [
                session_component_manifest(),
                options_component_manifest(),
                sdk_component_manifest(authority.clone()),
                echo_component_manifest(),
                consumer_component_manifest(authority.clone()),
            ],
            durable_schemas,
            [],
            &authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new(manifests).unwrap());
        kernel
            .register_embedded_factory(session.id, session_factory)
            .unwrap();
        kernel
            .register_embedded_factory(options.id, options_factory)
            .unwrap();
        kernel
            .register_embedded_factory(sdk.id, sdk_factory)
            .unwrap();
        kernel
            .register_embedded_factory(echo.id, || Box::new(EchoPlugin))
            .unwrap();
        kernel
            .register_embedded_factory(consumer.id, || Box::new(ConsumerPlugin))
            .unwrap();
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel.activate_all().unwrap();

        let output = kernel
            .invoke(&consumer_service(), &[], &authority, None)
            .unwrap();
        let output: ConsumerOutput = serde_json::from_slice(&output).unwrap();

        assert_eq!(
            output,
            ConsumerOutput {
                plugin: CONSUMER_PLUGIN.into(),
                settings: "configured".into(),
                state: 7,
                session: "root".into(),
                echo: "root".into(),
                echo_has_persistence_authority: false,
                has_persistence_authority: true,
            }
        );
    }
}

#[cfg(test)]
mod schema_match_tests {
    use phenix_core::{
        HasPhenixSchema, Key, PhenixSchema, PhenixValue, SchemaCompatibility, ValueMatch,
    };
    use phenix_sdk_macros::PhenixValue as DerivePhenixValue;
    use std::collections::BTreeMap;

    #[derive(Debug, Eq, PartialEq, DerivePhenixValue)]
    struct Needed {
        value: String,
    }

    #[derive(Debug, Eq, PartialEq, DerivePhenixValue)]
    struct Provided {
        value: String,
        extra: u64,
    }

    fn key(value: &str) -> Key {
        Key::parse(value).unwrap()
    }

    #[test]
    fn derived_schemas_classify_exact_and_projected_values() {
        assert!(matches!(
            Needed::phenix_schema().accepts(&Needed::phenix_schema()),
            SchemaCompatibility::Exact
        ));
        assert!(matches!(
            Needed::phenix_schema().accepts(&Provided::phenix_schema()),
            SchemaCompatibility::Compatible
        ));

        let exact = PhenixValue::from(&Needed {
            value: "exact".into(),
        });
        let matched: ValueMatch<Needed> = (&exact).into();
        assert!(matches!(matched, ValueMatch::Exact(_)));

        let projected = PhenixValue::from(&Provided {
            value: "projected".into(),
            extra: 7,
        });
        let matched: ValueMatch<Needed> = projected.into();
        assert!(matches!(matched, ValueMatch::Compatible(_)));

        let incompatible = PhenixValue::U64(7);
        assert!(matches!(
            incompatible.match_as::<Needed>(),
            ValueMatch::Incompatible(_)
        ));
    }

    #[test]
    fn variant_compatibility_is_directional() {
        let narrow = PhenixSchema::Variant(BTreeMap::from([(key("A"), PhenixSchema::Unit)]));
        let broad = PhenixSchema::Variant(BTreeMap::from([
            (key("A"), PhenixSchema::Unit),
            (key("B"), PhenixSchema::Unit),
        ]));

        assert!(matches!(
            broad.accepts(&narrow),
            SchemaCompatibility::Compatible
        ));
        assert!(matches!(
            narrow.accepts(&broad),
            SchemaCompatibility::Incompatible(_)
        ));
    }

    #[test]
    fn fixed_arrays_can_satisfy_dynamic_lists_but_not_the_reverse() {
        let array = PhenixSchema::Array {
            item: Box::new(PhenixSchema::String),
            len: 2,
        };
        let list = PhenixSchema::List(Box::new(PhenixSchema::String));

        assert!(matches!(
            list.accepts(&array),
            SchemaCompatibility::Compatible
        ));
        assert!(matches!(
            array.accepts(&list),
            SchemaCompatibility::Incompatible(_)
        ));
    }
}
