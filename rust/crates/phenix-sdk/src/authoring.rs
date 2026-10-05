//! Rust-native static plugin authoring.
//!
//! A static plugin has one concrete runtime identity, so generic plugin
//! declarations are rejected at compile time.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("phenix.example")]
//! struct Plugin<T> {
//!     marker: std::marker::PhantomData<T>,
//! }
//! ```
//!
//! Components likewise have one concrete static definition.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Api<T> {
//!     state: T,
//! }
//! ```
//!
//! Annotated dependency and component fields must name types that implement
//! the matching static authoring contract. Invalid wiring fails during
//! compilation rather than being deferred to graph construction.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("phenix.invalid-dependency")]
//! struct Plugin {
//!     #[phenix(dep)]
//!     dependency: u64,
//! }
//! ```
//!
//! Direct dependency namespaces expose only the dependency declared by that
//! plugin. Transitive dependencies stay under their owning plugin namespace.
//!
//! ```compile_fail
//! mod leaf {
//!     #[phenix_sdk::plugin("phenix.namespace.leaf")]
//!     pub struct Plugin;
//! }
//!
//! mod middle {
//!     #[phenix_sdk::plugin("phenix.namespace.middle")]
//!     pub struct Plugin {
//!         #[phenix(dep)]
//!         leaf: super::leaf::Plugin,
//!     }
//! }
//!
//! #[phenix_sdk::plugin("phenix.namespace.root")]
//! struct Plugin {
//!     #[phenix(dep)]
//!     middle: middle::Plugin,
//! }
//!
//! fn flattened(_: plugin::dependencies::leaf::Plugin) {}
//! ```
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("phenix.invalid-component")]
//! struct Plugin {
//!     #[phenix(component)]
//!     component: u64,
//! }
//! ```
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("phenix.invalid-resource")]
//! struct Plugin {
//!     #[phenix(resource)]
//!     resource: u64,
//! }
//! ```
//!
//! ```compile_fail
//! struct MissingCodec;
//!
//! #[phenix_sdk::plugin("phenix.invalid-config")]
//! struct Plugin {
//!     #[phenix(config)]
//!     config: MissingCodec,
//! }
//! ```
//!
//! Cross-plugin imports require a canonical interface marker rather than a
//! locally inferred field identity.
//!
//! ```compile_fail
//! struct LocalInterface;
//!
//! #[phenix_sdk::component]
//! struct Api {
//!     #[phenix(import)]
//!     models: phenix_sdk::Required<phenix_sdk::Call<LocalInterface, String, String>>,
//! }
//! ```
//!
//! Types at structural call boundaries must expose Phenix schemas.
//!
//! ```compile_fail
//! #[phenix_sdk::interface("phenix.invalid.schema@1")]
//! struct Models;
//!
//! struct MissingSchema;
//!
//! #[phenix_sdk::component]
//! struct Api {
//!     #[phenix(import)]
//!     models: phenix_sdk::Required<phenix_sdk::Call<Models, MissingSchema, String>>,
//! }
//! ```
//!
//! Matching-policy wrappers are not arbitrarily composable. Optionality wraps
//! a call boundary, while structural matching policy belongs to its values.
//!
//! ```compile_fail
//! #[phenix_sdk::interface("phenix.invalid.wrapper@1")]
//! struct Models;
//!
//! #[phenix_sdk::component]
//! struct Api {
//!     #[phenix(import)]
//!     models: phenix_sdk::Required<
//!         phenix_sdk::Optional<phenix_sdk::Call<Models, String, String>>,
//!     >,
//! }
//! ```
//!
//! Plugin fields have one semantic role. Conflicting roles are rejected by
//! the authoring macro before any runtime metadata is generated.
//!
//! ```compile_fail
//! mod dependency {
//!     #[phenix_sdk::plugin("phenix.role-conflict.dependency")]
//!     pub struct Plugin;
//! }
//!
//! #[phenix_sdk::plugin("phenix.role-conflict")]
//! struct Plugin {
//!     #[phenix(dep)]
//!     #[phenix(config)]
//!     dependency: dependency::Plugin,
//! }
//! ```
//!
//! Exposed field names are one local segment, and authority belongs to callable
//! methods rather than recursive field mounts.
//!
//! ```compile_fail
//! struct Nested;
//!
//! #[phenix_sdk::plugin(root, id = "phenix.invalid-exposed-name")]
//! struct Plugin {
//!     #[phenix(expose(name = "nested/path"))]
//!     nested: Nested,
//! }
//! ```
//!
//! ```compile_fail
//! struct Nested;
//!
//! #[phenix_sdk::plugin(root, id = "phenix.invalid-exposed-authority")]
//! struct Plugin {
//!     #[phenix(expose(authority = phenix_sdk::Authority::default()))]
//!     nested: Nested,
//! }
//! ```
//!
//! A selected field must provide a relative static exposure projection.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin(root, id = "phenix.invalid-exposed-type")]
//! struct Plugin {
//!     #[phenix(expose)]
//!     value: u64,
//! }
//!
//! #[phenix_sdk::plugin]
//! impl Plugin {
//!     #[phenix(expose)]
//!     fn run(&mut self) {}
//! }
//! ```
//!
//! A plugin also has one configuration owner. Two configuration fields are a
//! statically invalid declaration rather than a runtime merge problem.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("phenix.duplicate-config")]
//! struct Plugin {
//!     #[phenix(config)]
//!     primary: String,
//!     #[phenix(config)]
//!     fallback: String,
//! }
//! ```
//!
//! Explicit nested IDs must remain unique within their owning plugin.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct First;
//! #[phenix_sdk::component]
//! impl First {}
//!
//! #[phenix_sdk::component]
//! struct Second;
//! #[phenix_sdk::component]
//! impl Second {}
//!
//! #[phenix_sdk::plugin("phenix.duplicate-component")]
//! struct Plugin {
//!     #[phenix(component, id = "phenix.duplicate-component.api")]
//!     first: First,
//!     #[phenix(component, id = "phenix.duplicate-component.api")]
//!     second: Second,
//! }
//! ```
//!
//! Lifecycle methods require mutable plugin state and one context parameter.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("phenix.invalid-lifecycle")]
//! struct Plugin;
//!
//! #[phenix_sdk::plugin]
//! impl Plugin {
//!     #[phenix(start)]
//!     fn start(&self, _context: ()) -> Result<(), ()> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Generic lifecycle methods cannot be lowered into one concrete runtime
//! callback ABI.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("phenix.invalid-generic-lifecycle")]
//! struct GenericLifecycle;
//!
//! #[phenix_sdk::plugin]
//! impl GenericLifecycle {
//!     #[phenix(start)]
//!     fn start<T>(&mut self, _context: &phenix_sdk::PluginContext<'_, '_, ()>) -> Result<(), ()> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Component listeners receive `&EventContext` followed by exactly one typed event payload.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Api;
//!
//! #[phenix_sdk::component]
//! impl Api {
//!     #[phenix(listen("phenix.invalid.listener"))]
//!     fn created(
//!         &mut self,
//!         _context: &phenix_sdk::EventContext,
//!         _first: String,
//!         _second: String,
//!     ) {}
//! }
//! ```
//!
//! Component exports accept at most one request after an optional call
//! context.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Api;
//!
//! #[phenix_sdk::component]
//! impl Api {
//!     #[phenix(export("phenix.invalid.export@1"))]
//!     fn run(&mut self, _first: String, _second: String) -> String {
//!         String::new()
//!     }
//! }
//! ```
//!
//! Component layers are instance behavior. They require a borrowed component
//! receiver so generated dispatch can bind them to component state.
//!
//! ```compile_fail
//! #[phenix_sdk::interface("phenix.invalid.layer@1")]
//! struct Models;
//!
//! #[phenix_sdk::component]
//! struct Api;
//!
//! #[phenix_sdk::component]
//! impl Api {
//!     #[phenix(layer(Models, priority = 1))]
//!     fn policy(_request: String) {}
//! }
//! ```
//!
//! Public values are read-only projections. They may immutably inspect plugin
//! state, but a public value cannot borrow that state mutably.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Api;
//!
//! #[phenix_sdk::component]
//! impl Api {
//!     #[phenix(value("phenix.invalid.mutable-value@1"), public)]
//!     fn status(&mut self) -> u64 {
//!         1
//!     }
//! }
//! ```
//!
//! Public values also have one concrete projected type. Generic public values
//! cannot be represented in the resolved client projection.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Api;
//!
//! #[phenix_sdk::component]
//! impl Api {
//!     #[phenix(value("phenix.invalid.generic-value@1"), public)]
//!     fn status<T>(&self) -> T {
//!         todo!()
//!     }
//! }
//! ```
//!
//! Resource migrations must strictly advance the declared schema.
//!
//! ```compile_fail
//! struct Store;
//!
//! #[phenix_sdk::resource(schema = 2)]
//! impl Store {
//!     #[phenix(migrate(from = 2))]
//!     fn same_version(old: String) -> Result<String, ()> {
//!         Ok(old)
//!     }
//! }
//! ```
//!
//! Resource-only plugins cannot declare embedded component handlers. The
//! execution mode is known at expansion time, so this invalid combination is
//! rejected before trait adaptation or runtime graph construction.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Api;
//!
//! #[phenix_sdk::component]
//! impl Api {}
//!
//! #[phenix_sdk::plugin(
//!     id = "phenix.resource-only",
//!     execution = phenix_sdk::PluginExecution::ResourceOnly
//! )]
//! struct Plugin {
//!     #[phenix(component)]
//!     api: Api,
//! }
//! ```
//!
//! Plugin-root imports are embedded component behavior as well. A resource-only
//! plugin cannot smuggle dispatch behavior through root-field sugar.
//!
//! ```compile_fail
//! #[phenix_sdk::interface("phenix.resource-only.models@1")]
//! struct Models;
//!
//! #[phenix_sdk::plugin(
//!     id = "phenix.resource-only-import",
//!     execution = phenix_sdk::PluginExecution::ResourceOnly
//! )]
//! struct Plugin {
//!     #[phenix(import)]
//!     models: phenix_sdk::Required<phenix_sdk::Call<Models, String, String>>,
//! }
//! ```
//!
//! The stateless module form is an embedded-handler form. It is therefore
//! invalid for a resource-only plugin as well.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin(
//!     id = "phenix.resource-only-stateless",
//!     execution = phenix_sdk::PluginExecution::ResourceOnly
//! )]
//! mod plugin {}
//! ```
//!
//! Runtime-hosted plugins are metadata for an external runtime. They cannot
//! also declare handlers that require an embedded `PluginInstance` factory.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Api;
//!
//! #[phenix_sdk::component]
//! impl Api {}
//!
//! #[phenix_sdk::plugin(
//!     id = "phenix.runtime-hosted",
//!     execution = phenix_sdk::PluginExecution::Runtime {
//!         runtime: phenix_sdk::PluginRuntimeId::parse("fixture.runtime").unwrap(),
//!         artifact: phenix_sdk::PluginArtifact {
//!             locator: "fixture.wasm".into(),
//!             revision: phenix_sdk::ArtifactRevision::from_content(b"fixture"),
//!             configuration: std::collections::BTreeMap::new(),
//!         },
//!     }
//! )]
//! struct Plugin {
//!     #[phenix(component)]
//!     api: Api,
//! }
//! ```
//!
//! Runtime-hosted plugins likewise cannot declare plugin-root event emitters,
//! because that sugar creates an embedded root component.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin(
//!     id = "phenix.runtime-hosted-event",
//!     execution = phenix_sdk::PluginExecution::Runtime {
//!         runtime: phenix_sdk::PluginRuntimeId::parse("fixture.runtime").unwrap(),
//!         artifact: phenix_sdk::PluginArtifact {
//!             locator: "fixture.wasm".into(),
//!             revision: phenix_sdk::ArtifactRevision::from_content(b"fixture"),
//!             configuration: std::collections::BTreeMap::new(),
//!         },
//!     }
//! )]
//! struct Plugin {
//!     #[phenix(event("phenix.runtime-hosted.changed"))]
//!     changed: phenix_sdk::Emit<String>,
//! }
//! ```

mod context;
mod event_context;
mod plugin;
mod static_component;
mod static_config;
mod static_dispatch;
mod static_graph_runtime;
mod static_import;
mod static_lifecycle;
mod static_resource;

pub use context::*;
pub use event_context::EventContext;
pub use plugin::{
    EventEmitError, EventEmitter, EventName, StaticPluginDefinition, StaticPluginDependency,
    StaticPluginDescriptor, StaticPluginFactory, StaticPluginGraph, StaticPluginGraphError,
    TypedSdkClient,
};
pub use static_component::{
    InterfaceMarker, StaticComponentBehavior, StaticComponentDefinition, StaticComponentDescriptor,
    StaticComponentExport, StaticComponentLayer, StaticComponentListener, StaticComponentValue,
    StaticPluginComponents,
};
pub use static_config::{StaticPluginConfigDescriptor, StaticPluginConfiguration};
pub use static_dispatch::{
    LayerContext, StaticComponentDispatch, StaticComponentRuntimeDispatch,
    StaticPluginComponentDispatch, StaticPluginInstance, StaticPluginInvoke, StaticPluginStart,
    StaticPluginStop, block_on_static, decode_exact_runtime, decode_projected_runtime,
    encode_result_runtime, encode_runtime,
};
pub use static_import::{
    Call, Emit, Host, Optional, Required, StaticComponentEvent, StaticComponentHost,
    StaticComponentImport, StaticComponentImports, StaticEventField, StaticHostField,
    StaticImportField,
};
pub use static_lifecycle::{StaticPluginLifecycle, StaticPluginLifecycleDescriptor};
pub use static_resource::{
    Durable, StaticPluginResources, StaticResourceDefinition, StaticResourceDescriptor,
    StaticResourceField, StaticResourceMigration,
};

impl std::fmt::Debug for StaticPluginGraph {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.ids()).finish()
    }
}

#[doc(hidden)]
pub use plugin::{
    __phenix_plugin, HookName, ListenerDeclaration, ListenerProjection, dispatch_exact_provider,
    dispatch_projected_provider, listener_subscription, listener_subscription_with_authority,
};
