use crate::{
    CallableError, CallableInvocation, CallableRef, ComponentManifest, ContractId,
    InitialObservation, InterfaceId, ObservableError, ObservableStore, ObservationDelivery,
    ObservationMode, ObservationScope, ObservationSpec, PhenixSchema, PhenixValue, PluginId,
    PluginManifest, PluginRuntimeId, ReferenceGenerationId, ReferenceId, ReferenceOwnerId,
    ResolvedGeneration, SdkNamespace, SdkResourceId, SharedCallableRegistry, Type, ValueAddress,
    ValueChange, ValueId, ValuePath, ValuePathSegment,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
    sync::{Arc, Mutex},
};

const OBSERVABLE_GET_CONTRACT: &str = "phenix.observable-get@1";
const OBSERVABLE_LISTEN_CONTRACT: &str = "phenix.observable-listen@1";
const OBSERVABLE_DELIVERY_CONTRACT: &str = "phenix.observable-delivery@1";
const OBSERVABLE_STOP_CONTRACT: &str = "phenix.observable-stop@1";

/// A language-facing SDK is always published as one authoritative schema/value pair.
///
/// Callable values cannot recover their input and output schemas from a raw reference,
/// so consumers must retain this pair through every transport boundary.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SdkValue {
    pub schema: PhenixSchema,
    pub value: crate::PhenixValue,
}

impl SdkValue {
    pub fn new(
        schema: PhenixSchema,
        value: crate::PhenixValue,
    ) -> Result<Self, SdkResolutionError> {
        schema
            .parse(&value)
            .map_err(|error| SdkResolutionError::InvalidValue {
                message: error.to_string(),
            })?;
        Ok(Self { schema, value })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SdkObservableResource {
    pub id: SdkResourceId,
    pub binding_path: Vec<String>,
    pub value: ValueId,
    pub path: ValuePath,
    pub schema: PhenixSchema,
}

impl SdkObservableResource {
    #[must_use]
    pub fn new(
        id: SdkResourceId,
        binding_path: impl IntoIterator<Item = impl Into<String>>,
        value: ValueId,
        path: ValuePath,
        schema: PhenixSchema,
    ) -> Self {
        Self {
            id,
            binding_path: binding_path.into_iter().map(Into::into).collect(),
            value,
            path,
            schema,
        }
    }

    #[must_use]
    pub fn address(&self) -> ValueAddress {
        ValueAddress {
            value: self.value.clone(),
            path: self.path.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SdkContribution {
    pub provider: PluginId,
    pub namespace: SdkNamespace,
    pub interfaces: BTreeSet<InterfaceId>,
    pub resources: BTreeSet<SdkResourceId>,
    #[serde(default)]
    pub observables: BTreeMap<SdkResourceId, SdkObservableResource>,
    #[serde(default)]
    pub value: Option<SdkValue>,
}

impl SdkContribution {
    pub fn new(provider: PluginId, namespace: SdkNamespace) -> Self {
        Self {
            provider,
            namespace,
            interfaces: BTreeSet::new(),
            resources: BTreeSet::new(),
            observables: BTreeMap::new(),
            value: None,
        }
    }

    pub fn publish(&mut self, value: SdkValue) {
        self.value = Some(value);
    }

    pub fn insert_observable(&mut self, observable: SdkObservableResource) {
        self.resources.insert(observable.id.clone());
        self.observables.insert(observable.id.clone(), observable);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SdkResolutionError {
    InvalidValue {
        message: String,
    },
    UnknownProvider {
        namespace: SdkNamespace,
        provider: PluginId,
    },
    DuplicateNamespace(SdkNamespace),
    UnavailableInterface {
        namespace: SdkNamespace,
        interface: InterfaceId,
    },
    InvalidObservableResource {
        namespace: SdkNamespace,
        resource: SdkResourceId,
        message: String,
    },
    ConflictingBindingPath {
        namespace: SdkNamespace,
        path: Vec<String>,
    },
    CallableRegistration {
        namespace: SdkNamespace,
        resource: SdkResourceId,
        message: String,
    },
}

impl Display for SdkResolutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue { message } => {
                write!(f, "SDK value does not match its schema: {message}")
            }
            Self::UnknownProvider {
                namespace,
                provider,
            } => write!(
                f,
                "SDK namespace {namespace} references unselected provider {provider}"
            ),
            Self::DuplicateNamespace(namespace) => {
                write!(f, "multiple plugins provide SDK namespace {namespace}")
            }
            Self::UnavailableInterface {
                namespace,
                interface,
            } => write!(
                f,
                "SDK namespace {namespace} references unavailable interface {interface}"
            ),
            Self::InvalidObservableResource {
                namespace,
                resource,
                message,
            } => write!(
                f,
                "SDK namespace {namespace} observable resource {resource} is invalid: {message}"
            ),
            Self::ConflictingBindingPath { namespace, path } => write!(
                f,
                "SDK namespace {namespace} publishes multiple resources at binding path {}",
                path.join(".")
            ),
            Self::CallableRegistration {
                namespace,
                resource,
                message,
            } => write!(
                f,
                "SDK namespace {namespace} could not register observable resource \
                 {resource}: {message}"
            ),
        }
    }
}

impl Error for SdkResolutionError {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedSdkContributions {
    namespaces: BTreeMap<SdkNamespace, SdkContribution>,
}

impl ResolvedSdkContributions {
    pub fn resolve(
        plugins: &[PluginManifest],
        components: &[ComponentManifest],
        contributions: impl IntoIterator<Item = SdkContribution>,
    ) -> Result<Self, SdkResolutionError> {
        let providers: BTreeSet<_> = plugins.iter().map(|plugin| plugin.id.clone()).collect();
        let interfaces: BTreeSet<_> = components
            .iter()
            .flat_map(|component| component.exports.iter())
            .map(|export| export.interface.clone())
            .collect();
        let mut namespaces = BTreeMap::new();

        for contribution in contributions {
            if !providers.contains(&contribution.provider) {
                return Err(SdkResolutionError::UnknownProvider {
                    namespace: contribution.namespace,
                    provider: contribution.provider,
                });
            }
            if let Some(interface) = contribution
                .interfaces
                .iter()
                .find(|interface| !interfaces.contains(*interface))
            {
                return Err(SdkResolutionError::UnavailableInterface {
                    namespace: contribution.namespace,
                    interface: interface.clone(),
                });
            }
            let mut binding_paths = BTreeSet::new();
            for (resource, observable) in &contribution.observables {
                if resource != &observable.id || !contribution.resources.contains(resource) {
                    return Err(SdkResolutionError::InvalidObservableResource {
                        namespace: contribution.namespace.clone(),
                        resource: resource.clone(),
                        message: "observable identity must match and be present in resources"
                            .to_owned(),
                    });
                }
                if observable.binding_path.is_empty()
                    || observable.binding_path.iter().any(String::is_empty)
                {
                    return Err(SdkResolutionError::InvalidObservableResource {
                        namespace: contribution.namespace.clone(),
                        resource: resource.clone(),
                        message: "binding path must contain non-empty segments".to_owned(),
                    });
                }
                if !binding_paths.insert(observable.binding_path.clone()) {
                    return Err(SdkResolutionError::ConflictingBindingPath {
                        namespace: contribution.namespace.clone(),
                        path: observable.binding_path.clone(),
                    });
                }
            }
            if let Some(value) = &contribution.value {
                validate_capability_owners(&contribution.provider, &value.value)
                    .map_err(|message| SdkResolutionError::InvalidValue { message })?;
            }
            let namespace = contribution.namespace.clone();
            if namespaces.insert(namespace.clone(), contribution).is_some() {
                return Err(SdkResolutionError::DuplicateNamespace(namespace));
            }
        }

        Ok(Self { namespaces })
    }

    pub fn get(&self, namespace: &SdkNamespace) -> Option<&SdkContribution> {
        self.namespaces.get(namespace)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SdkNamespace, &SdkContribution)> {
        self.namespaces.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.namespaces.is_empty()
    }

    pub fn value(&self) -> Result<SdkValue, SdkResolutionError> {
        let mut schema = BTreeMap::new();
        let mut value = BTreeMap::new();
        for (namespace, contribution) in &self.namespaces {
            let Some(entry) = &contribution.value else {
                continue;
            };
            schema.insert(
                crate::Key::parse(namespace.as_str().to_owned())
                    .expect("SDK namespaces are structural keys"),
                entry.schema.clone(),
            );
            value.insert(
                crate::Key::parse(namespace.as_str().to_owned())
                    .expect("SDK namespaces are structural keys"),
                entry.value.clone(),
            );
        }
        SdkValue::new(Type::Table(schema), crate::PhenixValue::Table(value))
    }

    /// Materializes SDK observable resources as ordinary callable values.
    ///
    /// The runtime owns generated `get`, `listen`, and `stop` callables. The
    /// observable value itself remains owned by its contributing plugin.
    pub fn value_with_observables(
        &self,
        store: &ObservableStore,
        callables: &SharedCallableRegistry,
        runtime: &PluginRuntimeId,
        generation: ReferenceGenerationId,
    ) -> Result<SdkValue, SdkResolutionError> {
        self.validate_observables(store)?;

        let mut schema = BTreeMap::new();
        let mut value = BTreeMap::new();
        for (namespace, contribution) in &self.namespaces {
            let (mut namespace_schema, mut namespace_value) = contribution
                .value
                .as_ref()
                .map_or_else(empty_sdk_table, |entry| {
                    (entry.schema.clone(), entry.value.clone())
                });
            for observable in contribution.observables.values() {
                let (resource_schema, resource_value) =
                    observable_resource_value(runtime, &generation, observable);
                insert_binding(
                    namespace,
                    &mut namespace_schema,
                    &mut namespace_value,
                    &observable.binding_path,
                    resource_schema,
                    resource_value,
                )?;
            }
            schema.insert(
                crate::Key::parse(namespace.as_str().to_owned())
                    .expect("SDK namespaces are structural keys"),
                namespace_schema,
            );
            value.insert(
                crate::Key::parse(namespace.as_str().to_owned())
                    .expect("SDK namespaces are structural keys"),
                namespace_value,
            );
        }
        let sdk = SdkValue::new(Type::Table(schema), PhenixValue::Table(value))?;

        for (namespace, contribution) in &self.namespaces {
            for observable in contribution.observables.values() {
                register_observable_resource(
                    namespace,
                    runtime,
                    &generation,
                    observable,
                    store,
                    callables,
                )?;
            }
        }
        Ok(sdk)
    }

    pub fn validate_observables(&self, store: &ObservableStore) -> Result<(), SdkResolutionError> {
        for (namespace, contribution) in &self.namespaces {
            for (resource, observable) in &contribution.observables {
                let metadata = store.metadata(&observable.value).map_err(|error| {
                    SdkResolutionError::InvalidObservableResource {
                        namespace: namespace.clone(),
                        resource: resource.clone(),
                        message: error.to_string(),
                    }
                })?;
                if metadata.owner != contribution.provider {
                    return Err(SdkResolutionError::InvalidObservableResource {
                        namespace: namespace.clone(),
                        resource: resource.clone(),
                        message: format!(
                            "value {} is owned by {}, not {}",
                            observable.value, metadata.owner, contribution.provider
                        ),
                    });
                }
                let schema = store.schema(&observable.address()).map_err(|error| {
                    SdkResolutionError::InvalidObservableResource {
                        namespace: namespace.clone(),
                        resource: resource.clone(),
                        message: error.to_string(),
                    }
                })?;
                if schema != observable.schema {
                    return Err(SdkResolutionError::InvalidObservableResource {
                        namespace: namespace.clone(),
                        resource: resource.clone(),
                        message: "declared observable schema does not match the registered address"
                            .to_owned(),
                    });
                }
            }
        }
        Ok(())
    }
}

fn empty_sdk_table() -> (PhenixSchema, PhenixValue) {
    (
        Type::Table(BTreeMap::new()),
        PhenixValue::Table(BTreeMap::new()),
    )
}

fn observable_resource_value(
    runtime: &PluginRuntimeId,
    generation: &ReferenceGenerationId,
    observable: &SdkObservableResource,
) -> (PhenixSchema, PhenixValue) {
    let get_schema = observable_get_schema(&observable.schema);
    let listen_schema = observable_listen_schema();
    let get = observable_callable(
        runtime,
        generation,
        observable,
        "get",
        OBSERVABLE_GET_CONTRACT,
    );
    let listen = observable_callable(
        runtime,
        generation,
        observable,
        "listen",
        OBSERVABLE_LISTEN_CONTRACT,
    );
    (
        Type::Table(BTreeMap::from([
            (key("get"), get_schema),
            (key("listen"), listen_schema),
        ])),
        PhenixValue::Table(BTreeMap::from([
            (key("get"), PhenixValue::Callable(get)),
            (key("listen"), PhenixValue::Callable(listen)),
        ])),
    )
}

fn observable_get_schema(value: &PhenixSchema) -> Type {
    Type::Callable {
        contract: contract(OBSERVABLE_GET_CONTRACT),
        input: Box::new(Type::Unit),
        output: Box::new(Type::Table(BTreeMap::from([
            (key("value"), value.clone()),
            (key("version"), Type::U64),
        ]))),
    }
}

fn observable_listen_schema() -> Type {
    Type::Callable {
        contract: contract(OBSERVABLE_LISTEN_CONTRACT),
        input: Box::new(Type::Table(BTreeMap::from([
            (
                key("initial"),
                Type::Variant(BTreeMap::from([
                    (key("none"), Type::Unit),
                    (key("full"), Type::Unit),
                ])),
            ),
            (
                key("listener"),
                Type::Callable {
                    contract: contract(OBSERVABLE_DELIVERY_CONTRACT),
                    input: Box::new(observable_delivery_schema()),
                    output: Box::new(Type::Unit),
                },
            ),
            (
                key("mode"),
                Type::Variant(BTreeMap::from([
                    (key("diff"), Type::Unit),
                    (key("full"), Type::Unit),
                ])),
            ),
            (key("path"), observable_path_schema()),
            (
                key("scope"),
                Type::Variant(BTreeMap::from([
                    (key("exact"), Type::Unit),
                    (key("recursive"), Type::Unit),
                ])),
            ),
        ]))),
        output: Box::new(Type::Callable {
            contract: contract(OBSERVABLE_STOP_CONTRACT),
            input: Box::new(Type::Unit),
            output: Box::new(Type::Unit),
        }),
    }
}

/// Canonical structural schema for observable listener deliveries.
#[must_use]
pub fn observable_delivery_schema() -> Type {
    Type::Table(BTreeMap::from([
        (key("address"), observable_address_schema()),
        (key("commit_id"), Type::Option(Box::new(Type::U64))),
        (key("from_version"), Type::U64),
        (key("generation"), Type::U64),
        (key("payload"), observable_payload_schema()),
        (key("subscription_id"), Type::U64),
        (key("value_id"), Type::String),
        (key("version"), Type::U64),
    ]))
}

fn observable_address_schema() -> Type {
    Type::Table(BTreeMap::from([
        (key("path"), observable_path_schema()),
        (key("value_id"), Type::String),
    ]))
}

fn observable_path_schema() -> Type {
    Type::Table(BTreeMap::from([(
        key("segments"),
        Type::List(Box::new(Type::Variant(BTreeMap::from([
            (
                key("Field"),
                Type::Table(BTreeMap::from([(key("key"), Type::String)])),
            ),
            (
                key("Index"),
                Type::Table(BTreeMap::from([(key("index"), Type::U64)])),
            ),
            (
                key("MapKey"),
                Type::Table(BTreeMap::from([(key("key"), Type::String)])),
            ),
            (key("OptionPayload"), Type::Unit),
            (key("VariantPayload"), Type::Unit),
        ])))),
    )]))
}

fn observable_payload_schema() -> Type {
    Type::Variant(BTreeMap::from([
        (
            key("Diff"),
            Type::Table(BTreeMap::from([(
                key("changes"),
                Type::List(Box::new(observable_change_schema())),
            )])),
        ),
        (
            key("Full"),
            Type::Table(BTreeMap::from([(key("value"), Type::Any)])),
        ),
    ]))
}

fn observable_change_schema() -> Type {
    Type::Variant(BTreeMap::from([
        (
            key("Remove"),
            Type::Table(BTreeMap::from([(
                key("change"),
                Type::Table(BTreeMap::from([(key("path"), observable_path_schema())])),
            )])),
        ),
        (
            key("Replace"),
            Type::Table(BTreeMap::from([(
                key("change"),
                Type::Table(BTreeMap::from([
                    (key("path"), observable_path_schema()),
                    (key("value"), Type::Any),
                ])),
            )])),
        ),
        (
            key("Splice"),
            Type::Table(BTreeMap::from([(
                key("change"),
                Type::Table(BTreeMap::from([
                    (key("delete_count"), Type::U64),
                    (key("inserted"), Type::List(Box::new(Type::Any))),
                    (key("path"), observable_path_schema()),
                    (key("start"), Type::U64),
                ])),
            )])),
        ),
    ]))
}

fn observable_callable(
    runtime: &PluginRuntimeId,
    generation: &ReferenceGenerationId,
    observable: &SdkObservableResource,
    method: &str,
    contract_id: &str,
) -> CallableRef {
    CallableRef::new(
        contract(contract_id),
        ReferenceOwnerId::Runtime(runtime.clone()),
        generation.clone(),
        ReferenceId::parse(format!(
            "sdk/observable/{}/{method}",
            observable.id.as_str()
        ))
        .expect("SDK resource ids are valid callable reference segments"),
    )
}

fn register_observable_resource(
    namespace: &SdkNamespace,
    runtime: &PluginRuntimeId,
    generation: &ReferenceGenerationId,
    observable: &SdkObservableResource,
    store: &ObservableStore,
    callables: &SharedCallableRegistry,
) -> Result<(), SdkResolutionError> {
    let get = observable_callable(
        runtime,
        generation,
        observable,
        "get",
        OBSERVABLE_GET_CONTRACT,
    );
    let address = observable.address();
    let get_store = store.clone();
    register_callable(
        namespace,
        observable,
        callables,
        get,
        observable_get_schema(&observable.schema),
        move |_| {
            let (version, value) = get_store.get(&address).map_err(observable_handler_error)?;
            Ok(PhenixValue::Table(BTreeMap::from([
                (key("value"), value),
                (key("version"), PhenixValue::U64(version.get())),
            ])))
        },
    )?;

    let listen = observable_callable(
        runtime,
        generation,
        observable,
        "listen",
        OBSERVABLE_LISTEN_CONTRACT,
    );
    let listen_store = store.clone();
    let listen_callables = callables.clone();
    let base_address = observable.address();
    let resource = observable.id.clone();
    let stop_runtime = runtime.clone();
    let stop_generation = generation.clone();
    register_callable(
        namespace,
        observable,
        callables,
        listen,
        observable_listen_schema(),
        move |input| {
            let (listener, path, scope, mode, initial) = parse_listen_input(input)?;
            let address = ValueAddress {
                value: base_address.value.clone(),
                path: join_observable_path(&base_address.path, &path),
            };
            let callback_callables = listen_callables.clone();
            let callback = Arc::new(move |delivery: ObservationDelivery<'_>| {
                let input = observation_delivery_value(delivery);
                debug_assert!(observable_delivery_schema().parse(&input).is_ok());
                let _ = callback_callables.invoke(CallableInvocation {
                    callable: listener.clone(),
                    input,
                });
            });
            let subscription = listen_store
                .subscribe(
                    ObservationSpec {
                        address,
                        scope,
                        mode,
                        initial,
                    },
                    callback,
                )
                .map_err(observable_handler_error)?;
            let stop = CallableRef::new(
                contract(OBSERVABLE_STOP_CONTRACT),
                ReferenceOwnerId::Runtime(stop_runtime.clone()),
                stop_generation.clone(),
                ReferenceId::parse(format!(
                    "sdk/observable/{}/subscription/{}-{}",
                    resource.as_str(),
                    subscription.id.get(),
                    subscription.generation.get()
                ))
                .expect("subscription references use valid SDK resource ids"),
            );
            let stop_reference = Arc::new(Mutex::new(None));
            let handler_reference = Arc::clone(&stop_reference);
            let stop_store = listen_store.clone();
            let stop_callables = listen_callables.clone();
            let stop_registry = stop_callables.clone();
            let stop_for_handler = stop.clone();
            stop_callables.register(
                stop.clone(),
                Type::Callable {
                    contract: contract(OBSERVABLE_STOP_CONTRACT),
                    input: Box::new(Type::Unit),
                    output: Box::new(Type::Unit),
                },
                move |_| {
                    stop_store
                        .unsubscribe(&subscription)
                        .map_err(observable_handler_error)?;
                    if let Some(reference) = handler_reference
                        .lock()
                        .expect("stop callable lock poisoned")
                        .take()
                    {
                        stop_registry.unregister(&reference);
                    }
                    Ok(PhenixValue::Unit)
                },
            )?;
            *stop_reference.lock().expect("stop callable lock poisoned") = Some(stop_for_handler);
            Ok(PhenixValue::Callable(stop))
        },
    )
}

fn register_callable(
    namespace: &SdkNamespace,
    observable: &SdkObservableResource,
    callables: &SharedCallableRegistry,
    reference: CallableRef,
    schema: Type,
    handler: impl crate::CallableHandler + 'static,
) -> Result<(), SdkResolutionError> {
    callables
        .register(reference, schema, handler)
        .map_err(|error| SdkResolutionError::CallableRegistration {
            namespace: namespace.clone(),
            resource: observable.id.clone(),
            message: error.to_string(),
        })
}

fn parse_listen_input(
    input: PhenixValue,
) -> Result<
    (
        CallableRef,
        ValuePath,
        ObservationScope,
        ObservationMode,
        InitialObservation,
    ),
    CallableError,
> {
    let PhenixValue::Table(fields) = input else {
        return Err(CallableError::SchemaMismatch {
            message: "observable listen input must be a table".to_owned(),
        });
    };
    let listener = match fields.get("listener") {
        Some(PhenixValue::Callable(listener)) => listener.clone(),
        _ => {
            return Err(CallableError::SchemaMismatch {
                message: "observable listen input is missing listener".to_owned(),
            })
        }
    };
    Ok((
        listener,
        parse_observable_path(fields.get("path"))?,
        parse_scope(fields.get("scope"))?,
        parse_mode(fields.get("mode"))?,
        parse_initial(fields.get("initial"))?,
    ))
}

fn parse_observable_path(value: Option<&PhenixValue>) -> Result<ValuePath, CallableError> {
    let Some(PhenixValue::Table(fields)) = value else {
        return Err(CallableError::SchemaMismatch {
            message: "observable listen path must be a structural path".to_owned(),
        });
    };
    let Some(PhenixValue::List(segments)) = fields.get("segments") else {
        return Err(CallableError::SchemaMismatch {
            message: "observable listen path is missing segments".to_owned(),
        });
    };
    segments
        .iter()
        .map(parse_observable_path_segment)
        .collect::<Result<Vec<_>, _>>()
        .map(ValuePath::new)
}

fn parse_observable_path_segment(value: &PhenixValue) -> Result<ValuePathSegment, CallableError> {
    let PhenixValue::Variant { tag, value } = value else {
        return Err(CallableError::SchemaMismatch {
            message: "observable path segment must be a variant".to_owned(),
        });
    };
    match tag.as_str() {
        "Field" => {
            let PhenixValue::Table(fields) = value.as_ref() else {
                return Err(CallableError::SchemaMismatch {
                    message: "observable Field path segment must contain a table".to_owned(),
                });
            };
            let Some(PhenixValue::String(field)) = fields.get("key") else {
                return Err(CallableError::SchemaMismatch {
                    message: "observable Field path segment is missing key".to_owned(),
                });
            };
            crate::Key::parse(field.clone())
                .map(ValuePathSegment::Field)
                .map_err(|error| CallableError::SchemaMismatch {
                    message: error.to_owned(),
                })
        }
        "MapKey" => {
            let PhenixValue::Table(fields) = value.as_ref() else {
                return Err(CallableError::SchemaMismatch {
                    message: "observable MapKey path segment must contain a table".to_owned(),
                });
            };
            let Some(PhenixValue::String(map_key)) = fields.get("key") else {
                return Err(CallableError::SchemaMismatch {
                    message: "observable MapKey path segment is missing key".to_owned(),
                });
            };
            Ok(ValuePathSegment::MapKey(map_key.clone()))
        }
        "Index" => {
            let PhenixValue::Table(fields) = value.as_ref() else {
                return Err(CallableError::SchemaMismatch {
                    message: "observable Index path segment must contain a table".to_owned(),
                });
            };
            let Some(PhenixValue::U64(index)) = fields.get("index") else {
                return Err(CallableError::SchemaMismatch {
                    message: "observable Index path segment is missing index".to_owned(),
                });
            };
            u32::try_from(*index)
                .map(ValuePathSegment::Index)
                .map_err(|_| CallableError::SchemaMismatch {
                    message: "observable Index path segment exceeds u32".to_owned(),
                })
        }
        "VariantPayload" if matches!(value.as_ref(), PhenixValue::Unit) => {
            Ok(ValuePathSegment::VariantPayload)
        }
        "OptionPayload" if matches!(value.as_ref(), PhenixValue::Unit) => {
            Ok(ValuePathSegment::OptionPayload)
        }
        _ => Err(CallableError::SchemaMismatch {
            message: "observable path segment is invalid".to_owned(),
        }),
    }
}

fn join_observable_path(base: &ValuePath, relative: &ValuePath) -> ValuePath {
    ValuePath::new(
        base.segments()
            .iter()
            .cloned()
            .chain(relative.segments().iter().cloned()),
    )
}

fn parse_scope(value: Option<&PhenixValue>) -> Result<ObservationScope, CallableError> {
    match variant_tag(value)? {
        "exact" => Ok(ObservationScope::Exact),
        "recursive" => Ok(ObservationScope::Recursive),
        _ => Err(CallableError::SchemaMismatch {
            message: "observable listen scope is invalid".to_owned(),
        }),
    }
}

fn parse_mode(value: Option<&PhenixValue>) -> Result<ObservationMode, CallableError> {
    match variant_tag(value)? {
        "diff" => Ok(ObservationMode::Diff),
        "full" => Ok(ObservationMode::Full),
        _ => Err(CallableError::SchemaMismatch {
            message: "observable listen mode is invalid".to_owned(),
        }),
    }
}

fn parse_initial(value: Option<&PhenixValue>) -> Result<InitialObservation, CallableError> {
    match variant_tag(value)? {
        "none" => Ok(InitialObservation::None),
        "full" => Ok(InitialObservation::Full),
        _ => Err(CallableError::SchemaMismatch {
            message: "observable listen initial value is invalid".to_owned(),
        }),
    }
}

fn variant_tag(value: Option<&PhenixValue>) -> Result<&str, CallableError> {
    match value {
        Some(PhenixValue::Variant { tag, value })
            if matches!(value.as_ref(), PhenixValue::Unit) =>
        {
            Ok(tag.as_str())
        }
        _ => Err(CallableError::SchemaMismatch {
            message: "observable listen option must be a unit variant".to_owned(),
        }),
    }
}

fn observation_delivery_value(delivery: ObservationDelivery<'_>) -> PhenixValue {
    let payload = if delivery.snapshot.is_some() {
        let value = delivery
            .full_value()
            .expect("validated observable delivery address must resolve in its snapshot")
            .expect("delivery with a snapshot must produce a full value");
        variant_value(
            "Full",
            PhenixValue::Table(BTreeMap::from([(key("value"), value.clone())])),
        )
    } else {
        variant_value(
            "Diff",
            PhenixValue::Table(BTreeMap::from([(
                key("changes"),
                PhenixValue::List(
                    delivery
                        .changes
                        .iter()
                        .map(observation_change_value)
                        .collect(),
                ),
            )])),
        )
    };
    PhenixValue::Table(BTreeMap::from([
        (key("address"), observable_address_value(delivery.address)),
        (
            key("commit_id"),
            PhenixValue::Option(
                delivery
                    .commit
                    .map(|commit| Box::new(PhenixValue::U64(commit.get()))),
            ),
        ),
        (
            key("from_version"),
            PhenixValue::U64(delivery.from_version.get()),
        ),
        (
            key("generation"),
            PhenixValue::U64(delivery.generation.get()),
        ),
        (key("payload"), payload),
        (
            key("subscription_id"),
            PhenixValue::U64(delivery.observation.get()),
        ),
        (
            key("value_id"),
            PhenixValue::String(delivery.value.as_str().to_owned()),
        ),
        (key("version"), PhenixValue::U64(delivery.version.get())),
    ]))
}

fn observable_address_value(address: &ValueAddress) -> PhenixValue {
    PhenixValue::Table(BTreeMap::from([
        (key("path"), observable_path_value(&address.path)),
        (
            key("value_id"),
            PhenixValue::String(address.value.as_str().to_owned()),
        ),
    ]))
}

fn observable_path_value(path: &ValuePath) -> PhenixValue {
    PhenixValue::Table(BTreeMap::from([(
        key("segments"),
        PhenixValue::List(
            path.segments()
                .iter()
                .map(observable_path_segment_value)
                .collect(),
        ),
    )]))
}

fn observable_path_segment_value(segment: &ValuePathSegment) -> PhenixValue {
    match segment {
        ValuePathSegment::Field(field) => variant_value(
            "Field",
            PhenixValue::Table(BTreeMap::from([(
                key("key"),
                PhenixValue::String(field.as_str().to_owned()),
            )])),
        ),
        ValuePathSegment::MapKey(map_key) => variant_value(
            "MapKey",
            PhenixValue::Table(BTreeMap::from([(
                key("key"),
                PhenixValue::String(map_key.clone()),
            )])),
        ),
        ValuePathSegment::Index(index) => variant_value(
            "Index",
            PhenixValue::Table(BTreeMap::from([(
                key("index"),
                PhenixValue::U64(u64::from(*index)),
            )])),
        ),
        ValuePathSegment::VariantPayload => variant_value("VariantPayload", PhenixValue::Unit),
        ValuePathSegment::OptionPayload => variant_value("OptionPayload", PhenixValue::Unit),
    }
}

fn observation_change_value(change: &ValueChange) -> PhenixValue {
    match change {
        ValueChange::Replace { path, value } => variant_value(
            "Replace",
            PhenixValue::Table(BTreeMap::from([(
                key("change"),
                PhenixValue::Table(BTreeMap::from([
                    (key("path"), observable_path_value(path)),
                    (key("value"), value.clone()),
                ])),
            )])),
        ),
        ValueChange::Remove { path } => variant_value(
            "Remove",
            PhenixValue::Table(BTreeMap::from([(
                key("change"),
                PhenixValue::Table(BTreeMap::from([(key("path"), observable_path_value(path))])),
            )])),
        ),
        ValueChange::Splice {
            path,
            start,
            delete_count,
            inserted,
        } => variant_value(
            "Splice",
            PhenixValue::Table(BTreeMap::from([(
                key("change"),
                PhenixValue::Table(BTreeMap::from([
                    (
                        key("delete_count"),
                        PhenixValue::U64(u64::from(*delete_count)),
                    ),
                    (
                        key("inserted"),
                        PhenixValue::List(inserted.iter().cloned().collect()),
                    ),
                    (key("path"), observable_path_value(path)),
                    (key("start"), PhenixValue::U64(u64::from(*start))),
                ])),
            )])),
        ),
    }
}

fn variant_value(tag: &str, value: PhenixValue) -> PhenixValue {
    PhenixValue::Variant {
        tag: key(tag),
        value: Box::new(value),
    }
}

fn observable_handler_error(error: ObservableError) -> CallableError {
    CallableError::HandlerFailed {
        message: error.to_string(),
    }
}

fn insert_binding(
    namespace: &SdkNamespace,
    schema: &mut PhenixSchema,
    value: &mut PhenixValue,
    path: &[String],
    resource_schema: PhenixSchema,
    resource_value: PhenixValue,
) -> Result<(), SdkResolutionError> {
    let Some((head, tail)) = path.split_first() else {
        return Err(SdkResolutionError::ConflictingBindingPath {
            namespace: namespace.clone(),
            path: path.to_vec(),
        });
    };
    let Type::Table(schema_fields) = schema else {
        return Err(SdkResolutionError::ConflictingBindingPath {
            namespace: namespace.clone(),
            path: path.to_vec(),
        });
    };
    let PhenixValue::Table(value_fields) = value else {
        return Err(SdkResolutionError::ConflictingBindingPath {
            namespace: namespace.clone(),
            path: path.to_vec(),
        });
    };
    let head = key(head);
    if tail.is_empty() {
        if schema_fields.contains_key(&head) || value_fields.contains_key(&head) {
            return Err(SdkResolutionError::ConflictingBindingPath {
                namespace: namespace.clone(),
                path: path.to_vec(),
            });
        }
        schema_fields.insert(head.clone(), resource_schema);
        value_fields.insert(head, resource_value);
        return Ok(());
    }
    let (child_schema, child_value) =
        match (schema_fields.get_mut(&head), value_fields.get_mut(&head)) {
            (Some(child_schema), Some(child_value)) => (child_schema, child_value),
            (None, None) => {
                schema_fields.insert(head.clone(), Type::Table(BTreeMap::new()));
                value_fields.insert(head.clone(), PhenixValue::Table(BTreeMap::new()));
                (
                    schema_fields.get_mut(&head).expect("inserted schema field"),
                    value_fields.get_mut(&head).expect("inserted value field"),
                )
            }
            _ => {
                return Err(SdkResolutionError::ConflictingBindingPath {
                    namespace: namespace.clone(),
                    path: path.to_vec(),
                })
            }
        };
    insert_binding(
        namespace,
        child_schema,
        child_value,
        tail,
        resource_schema,
        resource_value,
    )
}

fn key(value: &str) -> crate::Key {
    crate::Key::parse(value.to_owned()).expect("static SDK key is valid")
}

fn contract(value: &str) -> ContractId {
    ContractId::parse(value).expect("static observable contract is valid")
}

fn validate_capability_owners(provider: &PluginId, value: &PhenixValue) -> Result<(), String> {
    match value {
        PhenixValue::Callable(reference) => match reference.owner() {
            ReferenceOwnerId::Plugin(owner) if owner == provider => Ok(()),
            ReferenceOwnerId::Plugin(owner) => Err(format!(
                "callable {} belongs to plugin {owner}, not contribution provider {provider}",
                reference.id()
            )),
            owner => Err(format!(
                "SDK contribution provider {provider} cannot publish {owner:?}-owned callable {}",
                reference.id()
            )),
        },
        PhenixValue::Object(reference) => match reference.owner() {
            ReferenceOwnerId::Plugin(owner) if owner == provider => Ok(()),
            ReferenceOwnerId::Plugin(owner) => Err(format!(
                "object {} belongs to plugin {owner}, not contribution provider {provider}",
                reference.id()
            )),
            owner => Err(format!(
                "SDK contribution provider {provider} cannot publish {owner:?}-owned object {}",
                reference.id()
            )),
        },
        PhenixValue::Option(Some(value)) | PhenixValue::Variant { value, .. } => {
            validate_capability_owners(provider, value)
        }
        PhenixValue::List(values) => values
            .iter()
            .try_for_each(|value| validate_capability_owners(provider, value)),
        PhenixValue::Map(values) => values
            .values()
            .try_for_each(|value| validate_capability_owners(provider, value)),
        PhenixValue::Table(values) => values
            .values()
            .try_for_each(|value| validate_capability_owners(provider, value)),
        PhenixValue::Unit
        | PhenixValue::Bool(_)
        | PhenixValue::I64(_)
        | PhenixValue::U64(_)
        | PhenixValue::F64(_)
        | PhenixValue::String(_)
        | PhenixValue::Bytes(_)
        | PhenixValue::Option(None) => Ok(()),
    }
}

impl ResolvedGeneration {
    pub fn resolve_sdk_contributions(
        &self,
        contributions: impl IntoIterator<Item = SdkContribution>,
    ) -> Result<ResolvedSdkContributions, SdkResolutionError> {
        ResolvedSdkContributions::resolve(self.plugins(), self.components(), contributions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Authority, CallableRef, ClientConnectionId, ComponentExport, ComponentId, ContractId, Key,
        ObservableRegistration, PhenixValue, PluginExecution, PluginRuntimeId,
        ReferenceGenerationId, ReferenceId, ReferenceOwnerId, SharedCallableRegistry,
        SnapshotPolicy, Type,
    };
    use std::sync::{Arc, Mutex};

    fn plugin(value: &str, execution: PluginExecution) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse(value).unwrap(),
            version: 1,
            execution,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn component(owner: &str, interface: &str) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse(format!("{owner}.component")).unwrap(),
            owner: PluginId::parse(owner).unwrap(),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: InterfaceId::parse(interface).unwrap(),
                schema: Default::default(),
                priority: 0,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        }
    }

    fn contribution(provider: &str, namespace: &str) -> SdkContribution {
        SdkContribution::new(
            PluginId::parse(provider).unwrap(),
            SdkNamespace::parse(namespace).unwrap(),
        )
    }

    fn listen_input(listener: CallableRef, path: &ValuePath) -> PhenixValue {
        PhenixValue::Table(BTreeMap::from([
            (key("listener"), PhenixValue::Callable(listener)),
            (
                key("scope"),
                PhenixValue::Variant {
                    tag: key("recursive"),
                    value: Box::new(PhenixValue::Unit),
                },
            ),
            (
                key("mode"),
                PhenixValue::Variant {
                    tag: key("diff"),
                    value: Box::new(PhenixValue::Unit),
                },
            ),
            (
                key("initial"),
                PhenixValue::Variant {
                    tag: key("full"),
                    value: Box::new(PhenixValue::Unit),
                },
            ),
            (key("path"), observable_path_value(path)),
        ]))
    }

    #[test]
    fn distinct_plugins_extend_distinct_sdk_namespaces() {
        let plugins = [
            plugin("phenix-sdk", PluginExecution::ResourceOnly),
            plugin("testing", PluginExecution::ResourceOnly),
        ];
        let resolved = ResolvedSdkContributions::resolve(
            &plugins,
            &[],
            [
                contribution("phenix-sdk", "phenix"),
                contribution("testing", "testing"),
            ],
        )
        .unwrap();

        assert_eq!(resolved.iter().count(), 2);
    }

    #[test]
    fn resource_only_plugin_can_publish_client_helpers() {
        let plugins = [plugin("testing", PluginExecution::ResourceOnly)];
        let mut testing = contribution("testing", "testing");
        testing
            .resources
            .insert(SdkResourceId::parse("sdk/rust/testing").unwrap());

        let resolved = ResolvedSdkContributions::resolve(&plugins, &[], [testing]).unwrap();

        assert!(resolved
            .get(&SdkNamespace::parse("testing").unwrap())
            .unwrap()
            .resources
            .contains(&SdkResourceId::parse("sdk/rust/testing").unwrap()));
    }

    #[test]
    fn sdk_interface_must_exist_in_selected_component_graph() {
        let plugins = [plugin("testing", PluginExecution::Embedded)];
        let mut testing = contribution("testing", "testing");
        testing
            .interfaces
            .insert(InterfaceId::parse("testing.inspect@1").unwrap());

        assert!(matches!(
            ResolvedSdkContributions::resolve(&plugins, &[], [testing]),
            Err(SdkResolutionError::UnavailableInterface { interface, .. })
                if interface == InterfaceId::parse("testing.inspect@1").unwrap()
        ));

        let mut testing = contribution("testing", "testing");
        testing
            .interfaces
            .insert(InterfaceId::parse("testing.inspect@1").unwrap());
        ResolvedSdkContributions::resolve(
            &plugins,
            &[component("testing", "testing.inspect@1")],
            [testing],
        )
        .unwrap();
    }

    #[test]
    fn duplicate_namespace_is_rejected() {
        let plugins = [
            plugin("testing-a", PluginExecution::ResourceOnly),
            plugin("testing-b", PluginExecution::ResourceOnly),
        ];

        assert!(matches!(
            ResolvedSdkContributions::resolve(
                &plugins,
                &[],
                [
                    contribution("testing-a", "testing"),
                    contribution("testing-b", "testing"),
                ],
            ),
            Err(SdkResolutionError::DuplicateNamespace(namespace))
                if namespace == SdkNamespace::parse("testing").unwrap()
        ));
    }

    #[test]
    fn contribution_provider_must_be_selected() {
        assert!(matches!(
            ResolvedSdkContributions::resolve(
                &[],
                &[],
                [contribution("testing", "testing")],
            ),
            Err(SdkResolutionError::UnknownProvider { provider, .. })
                if provider == PluginId::parse("testing").unwrap()
        ));
    }

    #[test]
    fn empty_composition_has_no_sdk_namespaces() {
        assert!(ResolvedSdkContributions::resolve(&[], &[], [])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn observable_metadata_is_validated_against_the_live_store() {
        let plugins = [plugin("testing", PluginExecution::ResourceOnly)];
        let mut testing = contribution("testing", "testing");
        testing.insert_observable(SdkObservableResource::new(
            SdkResourceId::parse("sdk/testing/state").unwrap(),
            ["state"],
            ValueId::parse("testing.state@1").unwrap(),
            ValuePath::root(),
            Type::U64,
        ));
        let resolved = ResolvedSdkContributions::resolve(&plugins, &[], [testing]).unwrap();
        let store = ObservableStore::default();
        store
            .register(ObservableRegistration {
                id: ValueId::parse("testing.state@1").unwrap(),
                owner: PluginId::parse("testing").unwrap(),
                schema: Type::U64,
                snapshot_policy: SnapshotPolicy::CurrentOnly,
                initial: PhenixValue::U64(0),
            })
            .unwrap();
        resolved.validate_observables(&store).unwrap();
    }

    #[test]
    fn observable_binding_paths_are_unique_within_a_namespace() {
        let plugins = [plugin("testing", PluginExecution::ResourceOnly)];
        let mut testing = contribution("testing", "testing");
        for resource in ["sdk/testing/first", "sdk/testing/second"] {
            testing.insert_observable(SdkObservableResource::new(
                SdkResourceId::parse(resource).unwrap(),
                ["state"],
                ValueId::parse(format!("testing.{resource}@1")).unwrap(),
                ValuePath::root(),
                Type::U64,
            ));
        }

        assert!(matches!(
            ResolvedSdkContributions::resolve(&plugins, &[], [testing]),
            Err(SdkResolutionError::ConflictingBindingPath { namespace, path })
                if namespace == SdkNamespace::parse("testing").unwrap()
                    && path == vec!["state".to_owned()]
        ));
    }

    #[test]
    fn sdk_value_cannot_publish_a_value_outside_its_authoritative_schema() {
        assert!(SdkValue::new(Type::U64, PhenixValue::U64(1)).is_ok());
        assert!(matches!(
            SdkValue::new(Type::U64, PhenixValue::String("wrong".to_owned())),
            Err(SdkResolutionError::InvalidValue { .. })
        ));
    }

    #[test]
    fn contributions_cannot_publish_other_owner_callables() {
        let plugins = [plugin("testing", PluginExecution::ResourceOnly)];
        let reference = CallableRef::new(
            ContractId::parse("testing.callback@1").unwrap(),
            ReferenceOwnerId::Client(ClientConnectionId::parse("nvim").unwrap()),
            ReferenceGenerationId::parse("connection-1").unwrap(),
            ReferenceId::parse("callback-1").unwrap(),
        );
        let schema = Type::Callable {
            contract: reference.contract().clone(),
            input: Box::new(Type::Unit),
            output: Box::new(Type::Unit),
        };
        let mut testing = contribution("testing", "testing");
        testing.publish(
            SdkValue::new(schema, PhenixValue::Callable(reference)).expect("matching value"),
        );

        assert!(matches!(
            ResolvedSdkContributions::resolve(&plugins, &[], [testing]),
            Err(SdkResolutionError::InvalidValue { message })
                if message.contains("cannot publish")
        ));
    }

    #[test]
    fn resolved_values_are_projected_under_stable_sdk_namespaces() {
        let plugins = [
            plugin("phenix-sdk", PluginExecution::ResourceOnly),
            plugin("testing", PluginExecution::ResourceOnly),
        ];
        let mut phenix = contribution("phenix-sdk", "phenix");
        phenix.publish(SdkValue::new(Type::U64, PhenixValue::U64(1)).unwrap());
        let mut testing = contribution("testing", "testing");
        testing
            .publish(SdkValue::new(Type::String, PhenixValue::String("ready".to_owned())).unwrap());

        let value = ResolvedSdkContributions::resolve(&plugins, &[], [testing, phenix])
            .unwrap()
            .value()
            .unwrap();

        assert_eq!(
            value.schema,
            Type::Table(BTreeMap::from([
                (Key::parse("phenix").unwrap(), Type::U64),
                (Key::parse("testing").unwrap(), Type::String),
            ]))
        );
        assert_eq!(
            value.value,
            PhenixValue::Table(BTreeMap::from([
                (Key::parse("phenix").unwrap(), PhenixValue::U64(1)),
                (
                    Key::parse("testing").unwrap(),
                    PhenixValue::String("ready".to_owned()),
                ),
            ]))
        );
    }

    #[test]
    fn observable_path_round_trips_every_structural_segment() {
        let path = ValuePath::new([
            ValuePathSegment::Field(Key::parse("field").unwrap()),
            ValuePathSegment::MapKey("map-key".to_owned()),
            ValuePathSegment::Index(7),
            ValuePathSegment::VariantPayload,
            ValuePathSegment::OptionPayload,
        ]);
        assert_eq!(
            parse_observable_path(Some(&observable_path_value(&path))).unwrap(),
            path
        );
    }

    #[test]
    fn observable_resources_materialize_as_generic_callable_values() {
        let plugins = [plugin("testing", PluginExecution::ResourceOnly)];
        let value_id = ValueId::parse("testing.state@1").unwrap();
        let mut testing = contribution("testing", "testing");
        testing.insert_observable(SdkObservableResource::new(
            SdkResourceId::parse("sdk/testing/state").unwrap(),
            ["state"],
            value_id.clone(),
            ValuePath::root(),
            Type::U64,
        ));
        let resolved = ResolvedSdkContributions::resolve(&plugins, &[], [testing]).unwrap();
        let store = ObservableStore::default();
        store
            .register(ObservableRegistration {
                id: value_id.clone(),
                owner: PluginId::parse("testing").unwrap(),
                schema: Type::U64,
                snapshot_policy: SnapshotPolicy::CopyOnChange,
                initial: PhenixValue::U64(1),
            })
            .unwrap();
        let callables = SharedCallableRegistry::default();
        let runtime = PluginRuntimeId::parse("phenix.sdk-runtime").unwrap();
        let generation = ReferenceGenerationId::parse("testing-generation").unwrap();
        let sdk = resolved
            .value_with_observables(&store, &callables, &runtime, generation.clone())
            .unwrap();
        let PhenixValue::Table(namespaces) = sdk.value else {
            panic!("SDK root is a table");
        };
        let PhenixValue::Table(resources) = namespaces.get("testing").unwrap() else {
            panic!("namespace is a table");
        };
        let PhenixValue::Table(state) = resources.get("state").unwrap() else {
            panic!("resource is a table");
        };
        let PhenixValue::Callable(get) = state.get("get").unwrap() else {
            panic!("get is callable");
        };
        assert_eq!(get.owner(), &ReferenceOwnerId::Runtime(runtime.clone()));
        assert_eq!(get.generation(), &generation);
        let get = callables
            .invoke(crate::CallableInvocation {
                callable: get.clone(),
                input: PhenixValue::Unit,
            })
            .unwrap();
        assert_eq!(
            get.output,
            PhenixValue::Table(BTreeMap::from([
                (Key::parse("value").unwrap(), PhenixValue::U64(1)),
                (Key::parse("version").unwrap(), PhenixValue::U64(0)),
            ]))
        );

        let PhenixValue::Callable(listen) = state.get("listen").unwrap() else {
            panic!("listen is callable");
        };
        assert_eq!(listen.owner(), &ReferenceOwnerId::Runtime(runtime));
        assert_eq!(listen.generation(), &generation);
        let listener = CallableRef::new(
            ContractId::parse(OBSERVABLE_DELIVERY_CONTRACT).unwrap(),
            ReferenceOwnerId::Client(ClientConnectionId::parse("test-client").unwrap()),
            ReferenceGenerationId::parse("connection-1").unwrap(),
            ReferenceId::parse("listener-1").unwrap(),
        );
        let deliveries = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&deliveries);
        callables
            .register(
                listener.clone(),
                Type::Callable {
                    contract: ContractId::parse(OBSERVABLE_DELIVERY_CONTRACT).unwrap(),
                    input: Box::new(observable_delivery_schema()),
                    output: Box::new(Type::Unit),
                },
                move |input| {
                    observable_delivery_schema().parse(&input).unwrap();
                    recorded.lock().unwrap().push(input);
                    Ok(PhenixValue::Unit)
                },
            )
            .unwrap();
        let stop = callables
            .invoke(crate::CallableInvocation {
                callable: listen.clone(),
                input: listen_input(listener, &ValuePath::root()),
            })
            .unwrap();
        let PhenixValue::Callable(stop) = stop.output else {
            panic!("listen returns a callable stop handle");
        };
        assert_eq!(deliveries.lock().unwrap().len(), 1);
        store
            .transaction([&value_id], |transaction| {
                transaction.replace(&value_id, ValuePath::root(), PhenixValue::U64(2))
            })
            .unwrap();
        {
            let deliveries = deliveries.lock().unwrap();
            assert_eq!(deliveries.len(), 2);
            observable_delivery_schema().parse(&deliveries[0]).unwrap();
            observable_delivery_schema().parse(&deliveries[1]).unwrap();
            let PhenixValue::Table(initial) = &deliveries[0] else {
                panic!("initial delivery is a table");
            };
            assert_eq!(initial.get("commit_id"), Some(&PhenixValue::Option(None)));
            let PhenixValue::Variant {
                tag: initial_payload,
                value: initial_payload_value,
            } = initial.get("payload").unwrap()
            else {
                panic!("initial payload is a variant");
            };
            assert_eq!(initial_payload.as_str(), "Full");
            assert_eq!(
                initial_payload_value.get("value").unwrap(),
                &PhenixValue::U64(1)
            );

            let PhenixValue::Table(update) = &deliveries[1] else {
                panic!("update delivery is a table");
            };
            assert_eq!(
                update.get("value_id"),
                Some(&PhenixValue::String(value_id.as_str().to_owned()))
            );
            assert_eq!(update.get("from_version"), Some(&PhenixValue::U64(0)));
            assert_eq!(update.get("version"), Some(&PhenixValue::U64(1)));
            assert_eq!(
                update.get("subscription_id"),
                initial.get("subscription_id")
            );
            assert_eq!(update.get("generation"), initial.get("generation"));
            let PhenixValue::Table(address) = update.get("address").unwrap() else {
                panic!("delivery address is a table");
            };
            assert_eq!(
                address.get("value_id"),
                Some(&PhenixValue::String(value_id.as_str().to_owned()))
            );
            assert_eq!(
                address.get("path"),
                Some(&observable_path_value(&ValuePath::root()))
            );
            let PhenixValue::Variant {
                tag: update_payload,
                value: update_payload_value,
            } = update.get("payload").unwrap()
            else {
                panic!("update payload is a variant");
            };
            assert_eq!(update_payload.as_str(), "Diff");
            let PhenixValue::List(changes) = update_payload_value.get("changes").unwrap() else {
                panic!("diff payload changes are a list");
            };
            assert_eq!(changes.len(), 1);
            let PhenixValue::Variant {
                tag: change_kind,
                value: change_value,
            } = &changes[0]
            else {
                panic!("diff change is a variant");
            };
            assert_eq!(change_kind.as_str(), "Replace");
            let PhenixValue::Table(change) = change_value.get("change").unwrap() else {
                panic!("replace change is a table");
            };
            assert_eq!(
                change.get("path"),
                Some(&observable_path_value(&ValuePath::root()))
            );
            assert_eq!(change.get("value"), Some(&PhenixValue::U64(2)));
        }
        callables
            .invoke(crate::CallableInvocation {
                callable: stop.clone(),
                input: PhenixValue::Unit,
            })
            .unwrap();
        store
            .transaction([&value_id], |transaction| {
                transaction.replace(&value_id, ValuePath::root(), PhenixValue::U64(3))
            })
            .unwrap();
        assert_eq!(deliveries.lock().unwrap().len(), 2);
        assert!(matches!(
            callables.invoke(crate::CallableInvocation {
                callable: stop,
                input: PhenixValue::Unit,
            }),
            Err(CallableError::UnknownReference(_))
        ));
    }

    #[test]
    fn listen_path_is_relative_to_the_resource_base_and_malformed_paths_do_not_subscribe() {
        let plugins = [plugin("testing", PluginExecution::ResourceOnly)];
        let value_id = ValueId::parse("testing.nested@1").unwrap();
        let outer = Key::parse("outer").unwrap();
        let inner = Key::parse("inner").unwrap();
        let root_schema = Type::Table(BTreeMap::from([(
            outer.clone(),
            Type::Table(BTreeMap::from([(inner.clone(), Type::U64)])),
        )]));
        let outer_schema = Type::Table(BTreeMap::from([(inner.clone(), Type::U64)]));
        let mut testing = contribution("testing", "testing");
        testing.insert_observable(SdkObservableResource::new(
            SdkResourceId::parse("sdk/testing/nested").unwrap(),
            ["nested"],
            value_id.clone(),
            ValuePath::new([ValuePathSegment::Field(outer.clone())]),
            outer_schema,
        ));
        let resolved = ResolvedSdkContributions::resolve(&plugins, &[], [testing]).unwrap();
        let store = ObservableStore::default();
        store
            .register(ObservableRegistration {
                id: value_id.clone(),
                owner: PluginId::parse("testing").unwrap(),
                schema: root_schema,
                snapshot_policy: SnapshotPolicy::CopyOnChange,
                initial: PhenixValue::Table(BTreeMap::from([(
                    outer.clone(),
                    PhenixValue::Table(BTreeMap::from([(inner.clone(), PhenixValue::U64(1))])),
                )])),
            })
            .unwrap();
        let callables = SharedCallableRegistry::default();
        let runtime = PluginRuntimeId::parse("phenix.sdk-runtime").unwrap();
        let sdk = resolved
            .value_with_observables(
                &store,
                &callables,
                &runtime,
                ReferenceGenerationId::parse("nested-generation").unwrap(),
            )
            .unwrap();
        let PhenixValue::Table(namespaces) = sdk.value else {
            panic!("SDK root is a table");
        };
        let PhenixValue::Table(resources) = namespaces.get("testing").unwrap() else {
            panic!("namespace is a table");
        };
        let PhenixValue::Table(nested) = resources.get("nested").unwrap() else {
            panic!("resource is a table");
        };
        let PhenixValue::Callable(listen) = nested.get("listen").unwrap() else {
            panic!("listen is callable");
        };
        let listener = CallableRef::new(
            contract(OBSERVABLE_DELIVERY_CONTRACT),
            ReferenceOwnerId::Client(ClientConnectionId::parse("test-client").unwrap()),
            ReferenceGenerationId::parse("connection-1").unwrap(),
            ReferenceId::parse("nested-listener").unwrap(),
        );
        let deliveries = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&deliveries);
        callables
            .register(
                listener.clone(),
                Type::Callable {
                    contract: contract(OBSERVABLE_DELIVERY_CONTRACT),
                    input: Box::new(observable_delivery_schema()),
                    output: Box::new(Type::Unit),
                },
                move |input| {
                    recorded.lock().unwrap().push(input);
                    Ok(PhenixValue::Unit)
                },
            )
            .unwrap();

        let mut malformed = match listen_input(listener.clone(), &ValuePath::root()) {
            PhenixValue::Table(fields) => fields,
            _ => unreachable!(),
        };
        malformed.insert(key("path"), PhenixValue::String("not-a-path".to_owned()));
        assert!(matches!(
            callables.invoke(crate::CallableInvocation {
                callable: listen.clone(),
                input: PhenixValue::Table(malformed),
            }),
            Err(CallableError::SchemaMismatch { .. })
        ));
        let absolute = ValuePath::new([
            ValuePathSegment::Field(outer.clone()),
            ValuePathSegment::Field(inner.clone()),
        ]);
        store
            .transaction([&value_id], |transaction| {
                transaction.replace(&value_id, absolute.clone(), PhenixValue::U64(2))
            })
            .unwrap();
        assert!(deliveries.lock().unwrap().is_empty());

        let relative = ValuePath::new([ValuePathSegment::Field(inner)]);
        let _stop = callables
            .invoke(crate::CallableInvocation {
                callable: listen.clone(),
                input: listen_input(listener, &relative),
            })
            .unwrap();
        store
            .transaction([&value_id], |transaction| {
                transaction.replace(&value_id, absolute.clone(), PhenixValue::U64(3))
            })
            .unwrap();
        let deliveries = deliveries.lock().unwrap();
        assert_eq!(deliveries.len(), 2);
        observable_delivery_schema().parse(&deliveries[1]).unwrap();
        let PhenixValue::Table(delivery) = &deliveries[1] else {
            panic!("delivery is a table");
        };
        let PhenixValue::Table(address) = delivery.get("address").unwrap() else {
            panic!("delivery address is a table");
        };
        assert_eq!(address.get("path"), Some(&observable_path_value(&absolute)));
    }
}
