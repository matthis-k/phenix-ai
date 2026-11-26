//! Public source traversal must not depend on crate-private error knowledge.
use phenix_graph::{
    Interface,
    component::ComponentDef,
    contract::Contract,
    network::{CompiledNetwork, NetworkBuilder, NetworkError},
    process::{Process, ProcessContext, Step},
};
use phenix_kernel::plugin_binding::{BindingFault, bind_component};
use phenix_plugin::{
    CallbackPhase, Code, Contribution, Failure, LifecycleFault, PluginCatalog, PluginInstance,
    PluginSet, PluginSnapshot, Registrar, RegistryFault, StagedPlugin,
};
use std::{
    any::TypeId,
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
};

fn cause<'a, T: Error + 'static>(mut error: &'a (dyn Error + 'static)) -> &'a T {
    loop {
        if let Some(cause) = error.downcast_ref::<T>() {
            return cause;
        }
        error = error
            .source()
            .expect("concrete public cause in source chain");
    }
}

fn check(error: &Failure, code: Code, message: &str) {
    assert_eq!(error.code, code);
    assert_eq!(error.message, message);
    assert!(!error.to_string().is_empty());
    assert!(!format!("{error:?}").is_empty());
}

struct Offer(&'static str, Option<String>);
impl Contribution for Offer {
    const KIND: &'static str = "test.offer";
    fn id(&self) -> &str {
        self.0
    }
}
struct OtherOffer;
impl Contribution for OtherOffer {
    const KIND: &'static str = Offer::KIND;
    fn id(&self) -> &str {
        "main"
    }
}
struct OtherKind;
impl Contribution for OtherKind {
    const KIND: &'static str = "test.other";
    fn id(&self) -> &str {
        "main"
    }
}
struct Provider(Option<String>);
impl PluginInstance for Provider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(Offer("main", self.0.clone()))
    }
}

#[test]
fn registry_and_catalog_validation_expose_the_existing_enum_and_captured_fields() {
    let error = Registrar::new("bad/owner").err().unwrap();
    check(&error, Code::DescriptorInvalid, "invalid owner");
    assert!(matches!(cause::<RegistryFault>(&error),
        RegistryFault::InvalidOwner { owner } if owner == "bad/owner"));

    let mut registrar = Registrar::new("provider").unwrap();
    let error = registrar.add(Offer("bad/id", None)).unwrap_err();
    check(
        &error,
        Code::DescriptorInvalid,
        "invalid contribution identity",
    );
    assert!(matches!(cause::<RegistryFault>(&error),
        RegistryFault::InvalidContribution { owner, kind, id }
        if owner == "provider" && *kind == Offer::KIND && id == "bad/id"));
    struct BadKind;
    impl Contribution for BadKind {
        const KIND: &'static str = "bad/kind";
        fn id(&self) -> &str {
            "valid"
        }
    }
    let error = registrar.add(BadKind).unwrap_err();
    assert!(matches!(cause::<RegistryFault>(&error),
        RegistryFault::InvalidContribution { owner, kind, id }
        if owner == "provider" && *kind == "bad/kind" && id == "valid"));

    registrar.add(Offer("main", None)).unwrap();
    let error = registrar.add(Offer("main", None)).unwrap_err();
    check(&error, Code::DescriptorInvalid, "duplicate contribution");
    assert!(matches!(cause::<RegistryFault>(&error),
        RegistryFault::DuplicateContribution { owner, kind, id }
        if owner == "provider" && *kind == Offer::KIND && id == "main"));

    let mut foreign = Registrar::new("foreign").unwrap();
    foreign.add(Offer("foreign", None)).unwrap();
    let error = registrar.merge(foreign.finish()).unwrap_err();
    check(
        &error,
        Code::DescriptorInvalid,
        "invalid bundle contribution identity or owner",
    );
    assert!(matches!(cause::<RegistryFault>(&error),
        RegistryFault::OwnerMismatch { expected, actual }
        if expected == "provider" && actual == "foreign"));
    assert_eq!(
        registrar.finish().keys().collect::<Vec<_>>(),
        [("provider", Offer::KIND, "main")]
    );

    let mut catalog = PluginCatalog::new();
    for name in ["root", "bad/plugin"] {
        let error = catalog
            .register(name, |path| Ok(Provider(path)))
            .err()
            .unwrap();
        check(&error, Code::DescriptorInvalid, "invalid plugin identity");
        assert!(matches!(cause::<RegistryFault>(&error),
            RegistryFault::InvalidPlugin { plugin } if plugin == name));
    }
    catalog
        .register("provider", |path| Ok(Provider(path)))
        .unwrap();
    let error = catalog
        .register("provider", |path| Ok(Provider(path)))
        .err()
        .unwrap();
    check(&error, Code::DescriptorInvalid, "duplicate plugin factory");
    assert!(matches!(cause::<RegistryFault>(&error),
        RegistryFault::DuplicatePlugin { plugin } if plugin == "provider"));
}

struct Identity;
impl Process for Identity {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        Ok(Step::Complete)
    }
}
fn contract() -> Contract {
    Contract::new("test/offer@1", Arc::new(Interface::new([], []).unwrap())).unwrap()
}
fn graph(snapshot: Option<&PluginSnapshot>, claimed: bool) -> CompiledNetwork {
    let required = contract();
    let mut definition =
        ComponentDef::primitive(required.required().clone(), Arc::new(|| Identity));
    if claimed {
        definition = definition.claim(&required).unwrap();
    }
    if let Some(snapshot) = snapshot {
        definition = definition.with_origin(
            phenix_graph::GraphOrigin::from_root(snapshot.revision_identity().clone()).child(),
        );
    }
    let mut builder = NetworkBuilder::new();
    builder.add("service", Arc::new(definition)).unwrap();
    builder.build().unwrap().compile().unwrap()
}

struct Dropped(Arc<AtomicUsize>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.fetch_add(1, SeqCst);
    }
}
struct LeasedProvider(Provider, Dropped);
impl PluginInstance for LeasedProvider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let _lease = &self.1;
        self.0.contribute(into)
    }
}

#[test]
fn public_binding_causes_keep_only_their_exact_revisions_and_preserve_network_sources() {
    let provider_drops = Arc::new(AtomicUsize::new(0));
    let code_drops = Arc::new(AtomicUsize::new(0));
    let unrelated_drops = Arc::new(AtomicUsize::new(0));
    let errors = {
        let mut catalog = PluginCatalog::new();
        let code = Dropped(code_drops.clone());
        let drops = provider_drops.clone();
        let key = catalog
            .register("provider", move |path| {
                let _code = &code;
                Ok(LeasedProvider(Provider(path), Dropped(drops.clone())))
            })
            .unwrap();
        let unrelated = Dropped(unrelated_drops.clone());
        let other = catalog
            .register("unrelated", move |path| {
                let _code = &unrelated;
                Ok(Provider(path))
            })
            .unwrap();
        let mut set = catalog.instance();
        set.load(&other, None).unwrap();
        let no_locator = set.load(&key, None).unwrap();
        let compiled = graph(None, true);
        let required = contract();
        let missing_locator = bind_component::<Offer>(
            &no_locator,
            "main",
            |offer| offer.1.as_deref(),
            &required,
            &compiled,
        )
            .err()
            .unwrap();
        let mismatch = bind_component::<OtherOffer>(
            &no_locator,
            "main",
            |_| None,
            &required,
            &compiled,
        )
            .err()
            .unwrap();
        let missing_kind = bind_component::<OtherKind>(
            &no_locator,
            "main",
            |_| None,
            &required,
            &compiled,
        )
            .err()
            .unwrap();
        let missing_id = bind_component::<Offer>(
            &no_locator,
            "absent",
            |offer| offer.1.as_deref(),
            &required,
            &compiled,
        )
            .err()
            .unwrap();
        let located = set.reload(&key, Some("service".into())).unwrap();
        let missing_origin = bind_component::<Offer>(
            &located,
            "main",
            |offer| offer.1.as_deref(),
            &required,
            &compiled,
        )
            .err()
            .unwrap();
        let old_graph = graph(Some(&located), true);
        let current = set.reload(&key, Some("service".into())).unwrap();
        let wrong_origin = bind_component::<Offer>(
            &current,
            "main",
            |offer| offer.1.as_deref(),
            &required,
            &old_graph,
        )
            .err()
            .unwrap();
        let missing_unit = bind_component::<Offer>(
            &current,
            "main",
            |offer| offer.1.as_deref(),
            &required,
            &NetworkBuilder::new().build().unwrap().compile().unwrap(),
        )
            .err()
            .unwrap();
        let unclaimed = graph(Some(&current), false);
        let schema = bind_component::<Offer>(
            &current,
            "main",
            |offer| offer.1.as_deref(),
            &required,
            &unclaimed,
        )
            .err()
            .unwrap();
        vec![
            missing_locator,
            mismatch,
            missing_kind,
            missing_id,
            missing_origin,
            wrong_origin,
            missing_unit,
            schema,
        ]
    };
    assert_eq!(unrelated_drops.load(SeqCst), 1);
    assert_eq!(provider_drops.load(SeqCst), 0);
    assert_eq!(code_drops.load(SeqCst), 0);
    for error in &errors {
        let context = error.context.as_ref().unwrap();
        assert_eq!(context.plugin, "provider");
        assert_eq!(context.phase, CallbackPhase::Bind);
        assert!(context.contribution.is_some());
        assert!(!error.to_string().is_empty());
        assert!(!format!("{error:?}").is_empty());
    }
    check(
        &errors[0],
        Code::MissingBinding,
        "contribution has no component locator",
    );
    assert!(matches!(cause::<BindingFault>(&errors[0]),
        BindingFault::MissingComponentLocator { plugin, revision: 2, kind, contribution }
        if plugin == "provider" && *kind == Offer::KIND && contribution == "main"));
    check(
        &errors[1],
        Code::MissingBinding,
        "missing contribution or type mismatch",
    );
    assert!(matches!(cause::<RegistryFault>(&errors[1]),
        RegistryFault::ContributionTypeMismatch { owner, kind, id, expected, actual }
        if owner == "provider" && *kind == Offer::KIND && id == "main"
        && *expected == TypeId::of::<OtherOffer>() && *actual == TypeId::of::<Offer>()));
    for (error, expected_kind, expected_id) in [
        (&errors[2], OtherKind::KIND, "main"),
        (&errors[3], Offer::KIND, "absent"),
    ] {
        check(
            error,
            Code::MissingBinding,
            "missing contribution or type mismatch",
        );
        assert!(matches!(cause::<RegistryFault>(error),
            RegistryFault::MissingContribution { owner, kind, id }
            if owner == "provider" && *kind == expected_kind && id == expected_id));
    }
    for (error, expected_revision, expected_origin) in
        [(&errors[4], 3, false), (&errors[5], 4, true)]
    {
        check(
            error,
            Code::Denied,
            "compiled unit service: contribution generation origin mismatch",
        );
        assert!(matches!(cause::<BindingFault>(error),
            BindingFault::GenerationOriginMismatch { plugin, revision, contribution, path, has_origin }
            if plugin == "provider" && *revision == expected_revision && contribution == "main"
            && path == "service" && *has_origin == expected_origin));
    }
    check(
        &errors[6],
        Code::MissingBinding,
        "compiled graph unit not found: service, contract test/offer@1",
    );
    assert_eq!(
        cause::<NetworkError>(&errors[6]),
        &NetworkError::UnknownProcess("service".into())
    );
    assert_eq!(errors[7].code, Code::SchemaRejected);
    let _original_network_error = cause::<NetworkError>(&errors[7]);

    // Each error group must pin only its originating revision, not its peers.
    let mut errors = errors.into_iter();
    for _ in 0..4 {
        drop(errors.next().unwrap());
    }
    assert_eq!(provider_drops.load(SeqCst), 1);
    assert_eq!(code_drops.load(SeqCst), 0);
    drop(errors.next().unwrap());
    assert_eq!(provider_drops.load(SeqCst), 2);
    assert_eq!(code_drops.load(SeqCst), 0);
    drop(errors);
    assert_eq!(provider_drops.load(SeqCst), 3);
    assert_eq!(code_drops.load(SeqCst), 1);
}

#[test]
fn lifecycle_preparation_checks_expose_public_causes_without_running_factories() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut catalog = PluginCatalog::new();
    let count = calls.clone();
    let key = catalog
        .register("provider", move |path| {
            count.fetch_add(1, SeqCst);
            Ok(Provider(path))
        })
        .unwrap();
    let mut old_set = catalog.instance();
    let late = catalog.register("late", |path| Ok(Provider(path))).unwrap();
    let mut foreign = PluginCatalog::new().instance();
    for error in [
        foreign.load(&key, None).err().unwrap(),
        foreign.prepare_reload(&key, None).err().unwrap(),
        foreign.prepare_unload(&key).err().unwrap(),
        foreign.snapshot(&key).err().unwrap(),
    ] {
        check(&error, Code::Denied, "foreign plugin catalog key");
        assert!(matches!(cause::<LifecycleFault>(&error),
            LifecycleFault::ForeignCatalogKey { plugin } if plugin == "provider"));
    }
    for error in [
        old_set.load(&late, None).err().unwrap(),
        old_set.prepare_reload(&late, None).err().unwrap(),
        old_set.prepare_unload(&late).err().unwrap(),
        old_set.snapshot(&late).err().unwrap(),
    ] {
        check(&error, Code::MissingBinding, "unregistered plugin");
        assert!(matches!(cause::<LifecycleFault>(&error),
            LifecycleFault::UnregisteredPlugin { plugin } if plugin == "late"));
    }
    for error in [
        old_set.prepare_reload(&key, None).err().unwrap(),
        old_set.prepare_unload(&key).err().unwrap(),
        old_set.snapshot(&key).err().unwrap(),
    ] {
        check(&error, Code::MissingBinding, "plugin is not loaded");
        assert!(matches!(cause::<LifecycleFault>(&error),
            LifecycleFault::NotLoaded { plugin } if plugin == "provider"));
    }
    assert_eq!(calls.load(SeqCst), 0);
    let original = old_set.load(&key, None).unwrap();
    let error = old_set.prepare_load(&key, None).err().unwrap();
    check(&error, Code::DescriptorInvalid, "plugin already loaded");
    assert!(matches!(cause::<LifecycleFault>(&error),
        LifecycleFault::AlreadyLoaded { plugin, revision: 1 } if plugin == "provider"));
    assert_eq!(calls.load(SeqCst), 1);
    assert_eq!(
        old_set.snapshot(&key).unwrap().revision(),
        original.revision()
    );
}

fn inventory(set: &PluginSet) -> Vec<(String, u64)> {
    set.snapshots()
        .map(|snapshot| (snapshot.name().into(), snapshot.revision()))
        .collect()
}

fn rejected_batch(
    set: &mut PluginSet,
    stages: Vec<StagedPlugin>,
    expected: LifecycleFault,
    message: &str,
) {
    let before = inventory(set);
    for error in [
        set.validate_batch(&stages).unwrap_err(),
        set.candidate_snapshots(&stages).err().unwrap(),
        set.commit_batch(stages).err().unwrap(),
    ] {
        check(&error, Code::Denied, message);
        assert_eq!(cause::<LifecycleFault>(&error), &expected);
    }
    assert_eq!(inventory(set), before);
}

#[test]
fn public_staging_rejections_preserve_validation_order_and_batch_atomicity() {
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("alpha", |path| Ok(Provider(path)))
        .unwrap();
    let b = catalog.register("beta", |path| Ok(Provider(path))).unwrap();
    let mut set = catalog.instance();
    set.commit_batch(vec![
        set.prepare_load(&a, None).unwrap(),
        set.prepare_load(&b, None).unwrap(),
    ])
    .unwrap();

    let duplicate = vec![
        set.prepare_unload(&a).unwrap(),
        set.prepare_reload(&a, None).unwrap(),
    ];
    rejected_batch(
        &mut set,
        duplicate,
        LifecycleFault::DuplicateStage {
            plugin: "alpha".into(),
        },
        "duplicate plugin stage",
    );
    let foreign = vec![
        set.prepare_reload(&a, None).unwrap(),
        catalog.instance().prepare_load(&b, None).unwrap(),
    ];
    rejected_batch(
        &mut set,
        foreign,
        LifecycleFault::ForeignSet {
            plugin: "beta".into(),
        },
        "stage belongs to a different plugin set",
    );

    let stale = set.prepare_reload(&a, None).unwrap();
    set.reload(&a, None).unwrap();
    let stages = vec![set.prepare_unload(&b).unwrap(), stale];
    rejected_batch(
        &mut set,
        stages,
        LifecycleFault::StaleStage {
            plugin: "alpha".into(),
            expected_epoch: 1,
            actual_epoch: 2,
            expected_revision: Some(1),
            actual_revision: Some(2),
        },
        "stale plugin stage",
    );

    let stale = set.prepare_unload(&a).unwrap();
    set.reload(&b, None).unwrap();
    rejected_batch(
        &mut set,
        vec![stale],
        LifecycleFault::StaleStage {
            plugin: "alpha".into(),
            expected_epoch: 2,
            actual_epoch: 3,
            expected_revision: Some(2),
            actual_revision: Some(2),
        },
        "stale plugin stage",
    );

    let stale = set.prepare_reload(&a, None).unwrap();
    set.unload(&a).unwrap();
    rejected_batch(
        &mut set,
        vec![stale],
        LifecycleFault::StaleStage {
            plugin: "alpha".into(),
            expected_epoch: 3,
            actual_epoch: 4,
            expected_revision: Some(2),
            actual_revision: None,
        },
        "stale plugin stage",
    );

    let stale = set.prepare_load(&a, None).unwrap();
    set.load(&a, None).unwrap();
    rejected_batch(
        &mut set,
        vec![stale],
        LifecycleFault::StaleStage {
            plugin: "alpha".into(),
            expected_epoch: 4,
            actual_epoch: 5,
            expected_revision: None,
            actual_revision: Some(5),
        },
        "stale plugin stage",
    );
    assert_eq!(set.snapshot(&a).unwrap().revision(), 5);
    assert_eq!(set.snapshot(&b).unwrap().revision(), 3);
}

#[test]
fn application_message_leaves_remain_valid_through_public_callback_wrappers() {
    struct Application;
    impl PluginInstance for Application {
        fn contribute(&self, _: &mut Registrar) -> Result<(), Failure> {
            // A genuine application callback need not define a closed cause enum.
            Err(Failure::new(Code::PluginFailed, "application contribution"))
        }
    }
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("application", |construct: bool| {
            if construct {
                // A genuine application factory may also return a message leaf.
                Err(Failure::new(
                    Code::SchemaRejected,
                    "application configuration",
                ))
            } else {
                Ok(Application)
            }
        })
        .unwrap();
    for (construct, phase, code, message) in [
        (
            true,
            CallbackPhase::Construct,
            Code::SchemaRejected,
            "application configuration",
        ),
        (
            false,
            CallbackPhase::Contribute,
            Code::PluginFailed,
            "application contribution",
        ),
    ] {
        let error = catalog
            .instance()
            .prepare_load(&key, construct)
            .err()
            .unwrap();
        check(&error, code, message);
        assert_eq!(error.context.as_ref().unwrap().phase, phase);
        let original = error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<Failure>()
            .unwrap();
        check(original, code, message);
        assert!(original.source().is_none());
    }
}
