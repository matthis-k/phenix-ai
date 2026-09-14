use phenix_application_interface::{
    ApplicationDescriptor, Cancel, Capabilities, CloseSession, CreateSession, Discover, ListModels,
    ListRoutingProfiles, ListSessions, Operation, Prompt, ResumeSession, SelectModel,
    SelectRoutingProfile,
};
use phenix_core::{ContractId, PhenixSchema};
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionMethod {
    pub method: String,
    pub operation: ContractId,
    pub capability: ContractId,
    pub input: PhenixSchema,
    pub output: PhenixSchema,
    pub error: PhenixSchema,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionEvent {
    pub method: String,
    pub event: ContractId,
    pub capability: ContractId,
    pub payload: PhenixSchema,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionCallback {
    pub method: String,
    pub callback: ContractId,
    pub capability: ContractId,
    pub request: PhenixSchema,
    pub response: PhenixSchema,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionCatalog {
    pub interface: ContractId,
    pub methods: Vec<ExtensionMethod>,
    pub events: Vec<ExtensionEvent>,
    pub callbacks: Vec<ExtensionCallback>,
}

#[must_use]
pub fn extension_catalog(
    descriptor: &ApplicationDescriptor,
    capabilities: &Capabilities,
) -> ExtensionCatalog {
    let methods = descriptor
        .operations
        .iter()
        .filter(|(id, operation)| {
            !is_standard_operation(id.as_str()) && supported(capabilities, &operation.capability)
        })
        .map(|(id, operation)| ExtensionMethod {
            method: extension_name(id),
            operation: id.clone(),
            capability: operation.capability.clone(),
            input: schema(descriptor, &operation.input),
            output: schema(descriptor, &operation.output),
            error: schema(descriptor, &operation.error),
        })
        .collect();

    let events = descriptor
        .events
        .iter()
        .filter(|(_, event)| supported(capabilities, &event.capability))
        .map(|(id, event)| ExtensionEvent {
            method: extension_name(id),
            event: id.clone(),
            capability: event.capability.clone(),
            payload: schema(descriptor, &event.payload),
        })
        .collect();

    let callbacks = descriptor
        .callbacks
        .iter()
        .filter(|(id, callback)| {
            !is_standard_callback(id.as_str()) && supported(capabilities, &callback.capability)
        })
        .map(|(id, callback)| ExtensionCallback {
            method: extension_name(id),
            callback: id.clone(),
            capability: callback.capability.clone(),
            request: schema(descriptor, &callback.request),
            response: schema(descriptor, &callback.response),
        })
        .collect();

    ExtensionCatalog {
        interface: descriptor.id.clone(),
        methods,
        events,
        callbacks,
    }
}

#[must_use]
pub fn extension_meta(
    descriptor: &ApplicationDescriptor,
    capabilities: &Capabilities,
) -> Map<String, Value> {
    let catalog = extension_catalog(descriptor, capabilities);
    let mut meta = Map::new();
    meta.insert(
        "phenix.extensions".to_owned(),
        json!({
            "interface": catalog.interface.as_str(),
            "capabilities": capabilities
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>(),
            "methods": catalog.methods.iter().map(|method| json!({
                "method": method.method,
                "operation": method.operation.as_str(),
                "capability": method.capability.as_str(),
                "input": method.input,
                "output": method.output,
                "error": method.error,
            })).collect::<Vec<_>>(),
            "events": catalog.events.iter().map(|event| json!({
                "method": event.method,
                "event": event.event.as_str(),
                "capability": event.capability.as_str(),
                "payload": event.payload,
            })).collect::<Vec<_>>(),
            "callbacks": catalog.callbacks.iter().map(|callback| json!({
                "method": callback.method,
                "callback": callback.callback.as_str(),
                "capability": callback.capability.as_str(),
                "request": callback.request,
                "response": callback.response,
            })).collect::<Vec<_>>(),
        }),
    );
    meta
}

fn supported(capabilities: &Capabilities, capability: &ContractId) -> bool {
    capabilities.iter().any(|item| item == capability)
}

fn schema(descriptor: &ApplicationDescriptor, id: &ContractId) -> PhenixSchema {
    descriptor
        .types
        .get(id)
        .unwrap_or_else(|| panic!("application descriptor is missing schema {id}"))
        .clone()
}

pub(crate) fn extension_name(id: &ContractId) -> String {
    let value = id.as_str();
    let suffix = value.strip_prefix("phenix.application.").unwrap_or(value);
    format!("_phenix/{suffix}")
}

fn is_standard_operation(id: &str) -> bool {
    [
        Discover::ID,
        CreateSession::ID,
        ListSessions::ID,
        ResumeSession::ID,
        CloseSession::ID,
        Prompt::ID,
        Cancel::ID,
        ListModels::ID,
        SelectModel::ID,
        ListRoutingProfiles::ID,
        SelectRoutingProfile::ID,
    ]
    .contains(&id)
}

fn is_standard_callback(id: &str) -> bool {
    id == "phenix.application.permission@1"
}
