use crate::{
    Authority, CallCancellationToken, ComponentId, ComponentInterface, ComponentInvocationError,
    DurableSchema, EventAdmissionReceipt, EventError, EventTypeId, Exact, GraphGenerationId,
    InterfaceId, KernelError, PhenixValue, PluginHost, PluginId, PreparedMutationHandle, Project,
    ResourceNamespace, SchemaMigration, ServiceId, TaskScope, TransactionOp, ValueError,
};
use std::marker::PhantomData;

const STRUCTURAL_MISMATCH_EVENT: &str = "kernel.structural_value_mismatch";
const STRUCTURAL_MISMATCH_EVENT_VERSION: u32 = 1;

/// Runtime view passed to plugin code.
///
/// The context separates generic kernel access, imported SDK contracts, current
/// plugin data, and current call data.
pub struct PluginContext<'host, 'runtime, Sdk, Settings = (), State = ()> {
    pub kernel: KernelAccess<'host, 'runtime>,
    pub sdk: Sdk,
    pub plugin: CurrentPlugin<'host, Settings, State>,
    pub call: CallContext<'host>,
}

impl<'host, 'runtime, Sdk, Settings, State> PluginContext<'host, 'runtime, Sdk, Settings, State> {
    pub fn new(
        host: &'host PluginHost<'runtime>,
        sdk: Sdk,
        settings: Settings,
        state: State,
    ) -> Self {
        Self {
            kernel: KernelAccess::new(host),
            sdk,
            plugin: CurrentPlugin {
                id: host.plugin(),
                settings,
                state,
            },
            call: CallContext {
                authority: host.authority(),
                graph_generation: host.graph_generation(),
            },
        }
    }
}

/// Data scoped to the current plugin instance.
pub struct CurrentPlugin<'host, Settings, State> {
    pub id: &'host PluginId,
    pub settings: Settings,
    pub state: State,
}

/// Data scoped to the current kernel-mediated call.
pub struct CallContext<'host> {
    pub authority: &'host Authority,
    pub graph_generation: Option<&'host GraphGenerationId>,
}

/// Scoped access to generic kernel mechanisms.
///
/// This wraps `PluginHost`; plugin business logic does not need the host itself.
#[derive(Clone, Copy)]
pub struct KernelAccess<'host, 'runtime> {
    host: &'host PluginHost<'runtime>,
}

impl<'host, 'runtime> KernelAccess<'host, 'runtime> {
    fn new(host: &'host PluginHost<'runtime>) -> Self {
        Self { host }
    }

    pub fn cancellation_token(&self) -> Option<&CallCancellationToken> {
        self.host.cancellation_token()
    }

    #[doc(hidden)]
    pub fn invoke_service_abi(
        &self,
        service: &ServiceId,
        input: &[u8],
        requested_authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        self.host
            .invoke_service_abi(service, input, requested_authority, binding)
    }

    pub fn continue_service(
        &self,
        input: &[u8],
        requested_authority: &Authority,
    ) -> Result<Vec<u8>, KernelError> {
        self.host.continue_service(input, requested_authority)
    }

    pub fn task_scope(&self) -> Option<TaskScope<'_>> {
        self.host.task_scope()
    }

    pub fn dispatch_event(
        &self,
        event_type: EventTypeId,
        version: u32,
        causality_id: u64,
        kernel_policy_revision: u64,
        payload: Vec<u8>,
    ) -> Result<EventAdmissionReceipt, EventError> {
        self.host.dispatch_event(
            event_type,
            version,
            causality_id,
            kernel_policy_revision,
            payload,
        )
    }

    /// Decode a structural request using the matching policy encoded in `T`.
    pub fn decode<T>(
        &self,
        interface: &InterfaceId,
        input: &[u8],
    ) -> Result<T, ComponentInvocationError>
    where
        for<'value> T: TryFrom<&'value PhenixValue, Error = ValueError>,
    {
        let value = serde_json::from_slice::<PhenixValue>(input)
            .map_err(|error| ComponentInvocationError::Decode(error.to_string()))?;
        T::try_from(&value).map_err(|error| {
            report_structural_mismatch(self.host, interface, "request", &error);
            ComponentInvocationError::Decode(error.to_string())
        })
    }

    /// Decode a structural request using the provider's local projected view.
    pub fn decode_projected<T>(
        &self,
        interface: &InterfaceId,
        input: &[u8],
    ) -> Result<T, ComponentInvocationError>
    where
        for<'value> T: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    {
        let value = serde_json::from_slice::<PhenixValue>(input)
            .map_err(|error| ComponentInvocationError::Decode(error.to_string()))?;
        T::try_from(Project(&value)).map_err(|error| {
            report_structural_mismatch(self.host, interface, "request", &error);
            ComponentInvocationError::Decode(error.to_string())
        })
    }

    /// Decode a structural request using the provider's exact local view.
    pub fn decode_exact<T>(
        &self,
        interface: &InterfaceId,
        input: &[u8],
    ) -> Result<T, ComponentInvocationError>
    where
        for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    {
        let value = serde_json::from_slice::<PhenixValue>(input)
            .map_err(|error| ComponentInvocationError::Decode(error.to_string()))?;
        T::try_from(Exact(&value)).map_err(|error| {
            report_structural_mismatch(self.host, interface, "request", &error);
            ComponentInvocationError::Decode(error.to_string())
        })
    }

    /// Encode a provider-local value for the structural plugin ABI.
    pub fn encode_value<T>(&self, value: &T) -> Result<Vec<u8>, ComponentInvocationError>
    where
        for<'value> PhenixValue: From<&'value T>,
    {
        serde_json::to_vec(&PhenixValue::from(value))
            .map_err(|error| ComponentInvocationError::Encode(error.to_string()))
    }

    pub fn register_durable_schema(&self, schema: &DurableSchema) -> Result<(), KernelError> {
        self.host.register_durable_schema(schema)
    }

    pub fn migrate_durable_schema(
        &self,
        schema: &DurableSchema,
        migrations: &[SchemaMigration],
    ) -> Result<(), KernelError> {
        self.host.migrate_durable_schema(schema, migrations)
    }

    pub fn read_durable(
        &self,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, KernelError> {
        self.host.read_durable(namespace, key)
    }

    pub fn transact_durable(
        &self,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<(), KernelError> {
        self.host.transact_durable(namespace, operations)
    }

    pub fn prepare_durable_transaction(
        &self,
        namespace: &ResourceNamespace,
        operations: &[TransactionOp],
    ) -> Result<PreparedMutationHandle, KernelError> {
        self.host.prepare_durable_transaction(namespace, operations)
    }

    pub fn transact_prepared(&self, handles: &[PreparedMutationHandle]) -> Result<(), KernelError> {
        self.host.transact_prepared(handles)
    }
}

/// Marker implemented by an SDK contract supplied by another plugin.
pub trait SdkContract {
    type Interface: ComponentInterface;
}

/// Kernel-mediated client for one imported interface.
pub struct SdkClient<'host, 'runtime, I: ComponentInterface> {
    host: &'host PluginHost<'runtime>,
    component: ComponentId,
    interface: PhantomData<fn() -> I>,
}

impl<'host, 'runtime, I: ComponentInterface> Clone for SdkClient<'host, 'runtime, I> {
    fn clone(&self) -> Self {
        Self {
            host: self.host,
            component: self.component.clone(),
            interface: PhantomData,
        }
    }
}

impl<'host, 'runtime, I: ComponentInterface> SdkClient<'host, 'runtime, I> {
    pub fn new(host: &'host PluginHost<'runtime>, component: ComponentId) -> Self {
        Self {
            host,
            component,
            interface: PhantomData,
        }
    }

    pub fn component(&self) -> &ComponentId {
        &self.component
    }

    /// Invoke the interface using the structural plugin ABI.
    pub fn invoke_value(
        &self,
        request: &PhenixValue,
    ) -> Result<PhenixValue, ComponentInvocationError> {
        self.host.invoke_import::<I>(&self.component, request)
    }

    /// Invoke and decode the response using the matching policy encoded in `Response`.
    pub fn invoke<Request, Response>(
        &self,
        request: &Request,
    ) -> Result<Response, ComponentInvocationError>
    where
        for<'value> PhenixValue: From<&'value Request>,
        for<'value> Response: TryFrom<&'value PhenixValue, Error = ValueError>,
    {
        let request = PhenixValue::from(request);
        let response = self.invoke_value(&request)?;
        Response::try_from(&response).map_err(|error| {
            report_structural_mismatch(self.host, &I::interface_id(), "response", &error);
            ComponentInvocationError::Decode(error.to_string())
        })
    }

    /// Invoke and let the consumer project the provider response into its local type.
    pub fn invoke_projected<Request, Response>(
        &self,
        request: &Request,
    ) -> Result<Response, ComponentInvocationError>
    where
        for<'value> PhenixValue: From<&'value Request>,
        for<'value> Response: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    {
        let request = PhenixValue::from(request);
        let response = self.invoke_value(&request)?;
        Response::try_from(Project(&response)).map_err(|error| {
            report_structural_mismatch(self.host, &I::interface_id(), "response", &error);
            ComponentInvocationError::Decode(error.to_string())
        })
    }

    /// Invoke and require the provider response to exactly match the consumer type.
    pub fn invoke_exact<Request, Response>(
        &self,
        request: &Request,
    ) -> Result<Response, ComponentInvocationError>
    where
        for<'value> PhenixValue: From<&'value Request>,
        for<'value> Response: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    {
        let request = PhenixValue::from(request);
        let response = self.invoke_value(&request)?;
        Response::try_from(Exact(&response)).map_err(|error| {
            report_structural_mismatch(self.host, &I::interface_id(), "response", &error);
            ComponentInvocationError::Decode(error.to_string())
        })
    }
}

fn report_structural_mismatch(
    host: &PluginHost<'_>,
    interface: &InterfaceId,
    direction: &'static str,
    error: &ValueError,
) {
    let payload = serde_json::json!({
        "interface": interface.as_str(),
        "direction": direction,
        "error": error.to_string(),
    });
    let Ok(payload) = serde_json::to_vec(&payload) else {
        return;
    };
    let event_type = EventTypeId::parse(STRUCTURAL_MISMATCH_EVENT)
        .expect("static structural mismatch event type is valid");
    let _ = host.dispatch_event(event_type, STRUCTURAL_MISMATCH_EVENT_VERSION, 0, 0, payload);
}

/// Consumer-side handle for a provider-owned SDK object.
///
/// The handle contains stable identity plus a scoped client. Provider state
/// remains owned by the providing plugin.
pub struct SdkObject<'host, 'runtime, I: ComponentInterface, Id = String> {
    id: Id,
    client: SdkClient<'host, 'runtime, I>,
}

impl<'host, 'runtime, I, Id> Clone for SdkObject<'host, 'runtime, I, Id>
where
    I: ComponentInterface,
    Id: Clone,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            client: self.client.clone(),
        }
    }
}

impl<'host, 'runtime, I: ComponentInterface, Id> SdkObject<'host, 'runtime, I, Id> {
    pub fn new(id: Id, client: SdkClient<'host, 'runtime, I>) -> Self {
        Self { id, client }
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn client(&self) -> &SdkClient<'host, 'runtime, I> {
        &self.client
    }

    pub fn into_id(self) -> Id {
        self.id
    }
}
