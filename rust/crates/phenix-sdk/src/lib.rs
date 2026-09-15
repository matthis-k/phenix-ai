#![forbid(unsafe_code)]

//! Rust-native plugin authoring rejects statically knowable invalid declarations.
//!
//! A static plugin has one concrete runtime identity, so generic plugin declarations fail during
//! macro expansion.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("fixture.generic")]
//! struct Plugin<T> {
//!     state: T,
//! }
//! ```
//!
//! A field has one Phenix role. Declaring two roles on the same field is rejected by the plugin
//! macro rather than producing ambiguous runtime wiring.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("fixture.duplicate-role")]
//! struct Plugin {
//!     #[phenix(dep)]
//!     #[phenix(component)]
//!     dependency: Dependency,
//! }
//!
//! struct Dependency;
//! ```
//!
//! Components are static runtime composition units. Generic component declarations fail before
//! they can produce type-dependent runtime identities.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Component<T> {
//!     state: T,
//! }
//! ```
//!
//! Layer declarations use the canonical default priority when ordering metadata is omitted.
//! Authors specify a priority only when relative layer ordering is semantic policy.
//!
//! Runtime-facing exports require a component receiver. A free-style method cannot silently lose
//! its component state and ownership context.
//!
//! ```compile_fail
//! struct Api;
//! #[phenix_sdk::component]
//! impl Api {
//!     #[phenix(export("fixture.compile-fail.export@1"))]
//!     fn run(request: String) -> String {
//!         request
//!     }
//! }
//! ```
//!
//! Event listeners borrow their component. Consuming the component would make subsequent
//! callbacks depend on invocation order, so the authoring macro rejects an owned receiver.
//!
//! ```compile_fail
//! struct Observer;
//! #[phenix_sdk::component]
//! impl Observer {
//!     #[phenix(listen("fixture.compile-fail.observed"))]
//!     fn observed(self, event: String) {
//!         let _ = event;
//!     }
//! }
//! ```
//!
//! Literal interface identities are versioned semantic contracts. Unversioned IDs are rejected at
//! compile time.
//!
//! ```compile_fail
//! struct Api;
//! #[phenix_sdk::component]
//! impl Api {
//!     #[phenix(export("fixture.compile-fail.unversioned"))]
//!     fn run(&mut self) {}
//! }
//! ```
//!
//! Public values are read-only projections. A public value cannot require a mutable component
//! borrow.
//!
//! ```compile_fail
//! struct Status;
//! #[phenix_sdk::component]
//! impl Status {
//!     #[phenix(value("fixture.compile-fail.status@1"), public)]
//!     fn status(&mut self) -> u64 {
//!         0
//!     }
//! }
//! ```
//!
//! Resource-only plugins cannot also declare embedded component behavior. The execution mode makes
//! that combination invalid before runtime composition.
//!
//! ```compile_fail
//! struct Api;
//! #[phenix_sdk::plugin(
//!     id = "fixture.compile-fail.resource-only",
//!     execution = phenix_sdk::PluginExecution::ResourceOnly
//! )]
//! struct Plugin {
//!     #[phenix(component)]
//!     api: Api,
//! }
//! ```
//!
//! Runtime-hosted plugins are hosted by a runtime provider and therefore cannot also carry
//! embedded component behavior.
//!
//! ```compile_fail
//! struct Api;
//! #[phenix_sdk::plugin(
//!     id = "fixture.compile-fail.runtime-hosted",
//!     execution = phenix_sdk::PluginExecution::Runtime {
//!         runtime: panic!(),
//!         artifact: panic!(),
//!     }
//! )]
//! struct Plugin {
//!     #[phenix(component)]
//!     api: Api,
//! }
//! ```
//!
//! The stateless module form is itself an embedded factory declaration. Runtime-hosted plugins
//! must be instantiated by their runtime provider instead.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin(
//!     id = "fixture.compile-fail.runtime-module",
//!     execution = phenix_sdk::PluginExecution::Runtime {
//!         runtime: panic!(),
//!         artifact: panic!(),
//!     }
//! )]
//! mod plugin {
//!     #[phenix(export("fixture.compile-fail.runtime-module.run@1"))]
//!     fn run(request: String) -> String {
//!         request
//!     }
//! }
//! ```
//!
//! Resource-only plugins likewise cannot use the stateless embedded-handler form because they
//! contribute resources rather than an embedded runtime instance.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin(
//!     id = "fixture.compile-fail.resource-module",
//!     execution = phenix_sdk::PluginExecution::ResourceOnly
//! )]
//! mod plugin {
//!     #[phenix(export("fixture.compile-fail.resource-module.run@1"))]
//!     fn run(request: String) -> String {
//!         request
//!     }
//! }
//! ```
//!
//! Lifecycle callbacks are synchronous kernel lifecycle boundaries. Async callbacks are rejected
//! rather than creating an unmanaged executor contract.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("fixture.compile-fail.async-lifecycle")]
//! struct Plugin;
//! #[phenix_sdk::plugin]
//! impl Plugin {
//!     #[phenix(start)]
//!     async fn start(
//!         &mut self,
//!         _context: &phenix_sdk::PluginContext<'_, '_, ()>,
//!     ) -> Result<(), String> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Typed configuration must provide a Phenix schema; an ordinary opaque Rust type is not enough.
//!
//! ```compile_fail
//! struct Settings;
//! #[phenix_sdk::plugin("fixture.compile-fail.config-schema")]
//! struct Plugin {
//!     #[phenix(config)]
//!     config: Settings,
//! }
//! ```
//!
//! Dependency fields must name an actual static plugin definition. Ordinary Rust values cannot
//! become composition dependencies merely by occupying a dependency field.
//!
//! ```compile_fail
//! struct NotAPlugin;
//! #[phenix_sdk::plugin("fixture.compile-fail.dependency-type")]
//! struct Plugin {
//!     #[phenix(dep)]
//!     dependency: NotAPlugin,
//! }
//! ```
//!
//! Component fields likewise require a real component definition so the kernel can derive canonical
//! runtime metadata without a parallel manifest.
//!
//! ```compile_fail
//! struct NotAComponent;
//! #[phenix_sdk::plugin("fixture.compile-fail.component-type")]
//! struct Plugin {
//!     #[phenix(component)]
//!     component: NotAComponent,
//! }
//! ```
//!
//! Nested plugin-owned identities must remain unique after derived and explicit IDs are resolved.
//!
//! ```compile_fail
//! struct Api;
//! struct CompatibilityApi;
//! #[phenix_sdk::plugin("fixture.compile-fail.duplicate-component")]
//! struct Plugin {
//!     #[phenix(component)]
//!     api: Api,
//!     #[phenix(component, id = "fixture.compile-fail.duplicate-component.api")]
//!     compatibility: CompatibilityApi,
//! }
//! ```
//!
//! Resource migrations can only start from an older schema. Impossible local migration metadata is
//! rejected before any runtime migration plan exists.
//!
//! ```compile_fail
//! struct Store;
//! struct V2;
//! struct MigrationError;
//! #[phenix_sdk::resource(schema = 2)]
//! impl Store {
//!     #[phenix(migrate(from = 2))]
//!     fn impossible(previous: V2) -> Result<V2, MigrationError> {
//!         Ok(previous)
//!     }
//! }
//! ```
//! Lifecycle callbacks use the resolved plugin context. A different context type cannot silently
//! bypass the kernel-owned lifecycle boundary.
//!
//! ```compile_fail
//! #[phenix_sdk::plugin("fixture.compile-fail.lifecycle")]
//! struct Plugin;
//! #[phenix_sdk::plugin]
//! impl Plugin {
//!     #[phenix(start)]
//!     fn start(&mut self, _context: &()) -> Result<(), String> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Structural wrappers carry exactly one payload type. Malformed wrappers fail during macro
//! expansion rather than creating an ambiguous matching policy.
//!
//! ```compile_fail
//! struct Observer;
//! #[phenix_sdk::component]
//! impl Observer {
//!     #[phenix(listen("fixture.compile-fail.structural"))]
//!     fn observed(&mut self, _event: phenix_sdk::Exact<String, u64>) {}
//! }
//! ```
//!
//! Cross-plugin imports require an interface marker that owns a canonical semantic identity.
//! Ordinary Rust types cannot stand in for that identity.
//!
//! ```compile_fail
//! #[phenix_sdk::component]
//! struct Api {
//!     #[phenix(import)]
//!     models: phenix_sdk::Required<phenix_sdk::Call<String, String, String>>,
//! }
//! ```
//!
//! One component method represents one runtime contribution. Combining export and listener roles
//! on the same method is rejected instead of creating two dispatch meanings for one Rust item.
//!
//! ```compile_fail
//! struct Api;
//! #[phenix_sdk::component]
//! impl Api {
//!     #[phenix(export("fixture.compile-fail.double-role@1"))]
//!     #[phenix(listen("fixture.compile-fail.double-role"))]
//!     fn run(&mut self, request: String) -> String {
//!         request
//!     }
//! }
//! ```
//!
//! Authority is one semantic modifier per contribution. Repeating it is rejected rather than
//! relying on argument order to decide which capability set applies.
//!
//! ```compile_fail
//! #[derive(phenix_sdk::PhenixValue)]
//! struct Request {
//!     value: String,
//! }
//!
//! #[phenix_sdk::interface("fixture.compile-fail.models@1")]
//! struct Models;
//!
//! #[phenix_sdk::component]
//! struct Api {
//!     #[phenix(
//!         import,
//!         authority = phenix_sdk::Authority::default(),
//!         authority = phenix_sdk::Authority::default()
//!     )]
//!     models: phenix_sdk::Required<phenix_sdk::Call<Models, Request, Request>>,
//! }
//! ```

mod api;
mod authoring;
pub mod contracts;
mod providers;
mod public_projection;

pub use api::*;
pub use authoring::*;
pub use contracts::*;
pub use phenix_core::{
    ArtifactRevision, Authority, BackendFeature, Bytes, CallableRef, CapabilityId, ComponentId,
    Contract, ContractId, ContractValue, DurableSchema, Exact, HasPhenixSchema, Key, LayerResult,
    ObjectRef, PhenixContract, PhenixSchema, PhenixValue, PluginArtifact, PluginExecution,
    PluginId, Project, ReferenceId, RuntimeId, Type, TypeKind, ValueError,
};
pub use phenix_provider_sdk::{
    ApiTokenSource, Auth, AuthDescriptor, AuthKind, EnvironmentVariable, ProviderAuthCommand,
    ProviderAuthInterface, ProviderAuthResponse, ProviderError, RateLimits,
};
pub use phenix_sdk_macros::{
    component, expose, interface, plugin, resource, PhenixContract, PhenixValue,
};
pub use providers::{Provider, ProviderSdkError, ProviderSdkExt, Providers};
pub use public_projection::*;

#[cfg(test)]
pub(crate) use phenix_plugin_api::{sdk_component_manifest, sdk_factory, sdk_manifest};

pub mod auth {
    pub use phenix_provider_sdk::auth::*;
}

pub mod provider {
    pub use phenix_provider_sdk::provider::*;
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::ValueCodec;
    use std::collections::BTreeMap;

    #[derive(Clone, Debug, PartialEq, PhenixValue, PhenixContract)]
    #[phenix(id = "fixture.coverage@1")]
    struct Coverage {
        covered: u64,
        total: u64,
        label: String,
        previous: Option<u64>,
    }

    #[derive(Clone, Debug, PartialEq, PhenixValue)]
    struct CoverageProjection {
        covered: u64,
        label: String,
    }

    #[derive(Clone, Debug, PartialEq, PhenixValue)]
    struct NestedProjection {
        coverage: CoverageProjection,
    }

    #[derive(Clone, Debug, PartialEq, PhenixValue)]
    enum RunResult {
        Passed { output: String },
        Failed(String),
        Cancelled,
    }

    #[derive(Clone, Debug, PartialEq, PhenixValue)]
    enum ErrorVariant {
        Error,
        Value(String),
    }

    fn key(value: &str) -> Key {
        Key::parse(value).unwrap()
    }

    #[test]
    fn derived_native_type_round_trips_through_core_only_contract_value() {
        let coverage = Coverage {
            covered: 90,
            total: 100,
            label: "unit".into(),
            previous: None,
        };
        let value = coverage.to_contract_value().unwrap();

        assert_eq!(value.contract().id().as_str(), "fixture.coverage@1");
        assert_eq!(value.get("covered").unwrap().value::<u64>().unwrap(), 90);
        assert_eq!(Coverage::from_contract_value(&value).unwrap(), coverage);
    }

    #[test]
    fn exact_try_from_rejects_extra_fields() {
        let value = PhenixValue::Table(BTreeMap::from([
            (key("covered"), PhenixValue::U64(90)),
            (key("total"), PhenixValue::U64(100)),
            (key("label"), PhenixValue::String("unit".into())),
            (key("previous"), PhenixValue::Option(None)),
            (key("unexpected"), PhenixValue::Bool(true)),
        ]));

        assert_eq!(
            Coverage::try_from(Exact(&value)).unwrap_err(),
            ValueError::UnexpectedKey(key("unexpected"))
        );
    }

    #[test]
    fn same_derived_type_supports_exact_and_projected_conversion() {
        let value = Coverage {
            covered: 90,
            total: 100,
            label: "unit".into(),
            previous: None,
        }
        .to_value();

        assert!(matches!(
            CoverageProjection::try_from(Exact(&value)),
            Err(ValueError::UnexpectedKey(_))
        ));
        assert_eq!(
            CoverageProjection::try_from(Project(&value)).unwrap(),
            CoverageProjection {
                covered: 90,
                label: "unit".into(),
            }
        );

        let missing = PhenixValue::Table(BTreeMap::from([(key("covered"), PhenixValue::U64(90))]));
        assert_eq!(
            CoverageProjection::try_from(Project(&missing)).unwrap_err(),
            ValueError::MissingKey(key("label"))
        );
    }

    #[test]
    fn projection_applies_recursively() {
        let value = PhenixValue::Table(BTreeMap::from([
            (
                key("coverage"),
                Coverage {
                    covered: 90,
                    total: 100,
                    label: "unit".into(),
                    previous: None,
                }
                .to_value(),
            ),
            (key("outer_extra"), PhenixValue::Bool(true)),
        ]));

        assert_eq!(
            NestedProjection::try_from(Project(&value)).unwrap(),
            NestedProjection {
                coverage: CoverageProjection {
                    covered: 90,
                    label: "unit".into(),
                },
            }
        );
    }

    #[test]
    fn error_named_variant_does_not_collide_with_try_from_error() {
        let value = ErrorVariant::Error.to_value();
        assert_eq!(
            ErrorVariant::try_from(Exact(&value)).unwrap(),
            ErrorVariant::Error
        );
        let value = ErrorVariant::Value("ok".into()).to_value();
        assert_eq!(
            ErrorVariant::try_from(Project(&value)).unwrap(),
            ErrorVariant::Value("ok".into())
        );
    }

    #[test]
    fn derived_enum_has_a_tagged_structural_shape() {
        let result = RunResult::Passed {
            output: "ok".into(),
        };
        let value = result.to_value();
        let (tag, payload) = value.variant().unwrap();

        assert_eq!(tag.as_str(), "Passed");
        assert_eq!(
            payload.get("output").unwrap().value::<String>().unwrap(),
            "ok"
        );
        assert_eq!(RunResult::try_from(Exact(&value)).unwrap(), result);

        assert_eq!(
            RunResult::try_from(Exact(&RunResult::Failed("boom".into()).to_value())).unwrap(),
            RunResult::Failed("boom".into())
        );
        assert_eq!(
            RunResult::try_from(Exact(&RunResult::Cancelled.to_value())).unwrap(),
            RunResult::Cancelled
        );
    }
}
