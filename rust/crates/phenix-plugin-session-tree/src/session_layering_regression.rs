use crate::{session_tree_factory, session_tree_manifest};
use phenix_core::{
    Authority, Kernel, KernelConfig, KernelError, LayerPolicy, PhenixValue, PluginId, Project,
    ServiceParticipantOutcome, SessionId,
};
use phenix_plugin_sessions::{
    session_factory, session_manifest, session_service, SessionCommand, SessionRecord,
    SessionResponse,
};

fn authority() -> Authority {
    Authority::new(
        session_manifest()
            .maximum_authority
            .capabilities()
            .cloned()
            .chain(
                session_tree_manifest()
                    .maximum_authority
                    .capabilities()
                    .cloned(),
            ),
    )
}

fn configured_kernel(layer: LayerPolicy) -> Kernel {
    let sessions = session_manifest();
    let tree = session_tree_manifest();
    let session_plugin = sessions.id.clone();
    let tree_plugin = tree.id.clone();
    let config = KernelConfig::new([sessions, tree])
        .unwrap()
        .with_layer_policy(session_service(), vec![layer])
        .unwrap();
    let mut kernel = Kernel::new(config);
    kernel
        .register_embedded_factory(session_plugin, session_factory)
        .unwrap();
    kernel
        .register_embedded_factory(tree_plugin, session_tree_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn create(kernel: &mut Kernel, id: &str) -> SessionResponse {
    let command = SessionCommand::Create {
        session: phenix_sdk::SessionRecord::new(SessionId::parse(id).unwrap()),
    };
    let output = kernel
        .invoke(
            &session_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &authority(),
            None,
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    SessionResponse::try_from(Project(&output)).unwrap()
}

#[test]
fn configured_session_tree_layer_delegates_to_flat_session_terminal() {
    let tree = session_tree_manifest().id;
    let sessions = session_manifest().id;
    let mut kernel = configured_kernel(LayerPolicy {
        plugin: tree.clone(),
        priority: 100,
        required: false,
        enabled: true,
    });

    assert_eq!(
        create(&mut kernel, "root"),
        SessionResponse::Created {
            session: SessionRecord::new(SessionId::parse("root").unwrap()),
        }
    );

    let provenance = kernel.service_invocation_provenance();
    let invocation = provenance.last().unwrap();
    assert_eq!(invocation.planned_chain.layers.len(), 1);
    assert_eq!(invocation.planned_chain.layers[0].plugin, tree);
    assert_eq!(invocation.planned_chain.terminal.plugin, sessions);
    assert_eq!(invocation.participants.len(), 2);
    assert_eq!(
        invocation.participants[0].outcome,
        ServiceParticipantOutcome::Delegated
    );
    assert_eq!(
        invocation.participants[1].outcome,
        ServiceParticipantOutcome::Succeeded
    );
}

#[test]
fn disabled_optional_session_tree_layer_leaves_basic_sessions_usable() {
    let tree = session_tree_manifest().id;
    let mut kernel = configured_kernel(LayerPolicy {
        plugin: tree,
        priority: 100,
        required: false,
        enabled: false,
    });

    assert!(matches!(
        create(&mut kernel, "root"),
        SessionResponse::Created { .. }
    ));
    assert!(kernel
        .service_invocation_provenance()
        .last()
        .unwrap()
        .planned_chain
        .layers
        .is_empty());
}

#[test]
fn required_session_tree_layer_fails_closed_when_unavailable() {
    let sessions = session_manifest();
    let tree = PluginId::parse("phenix.session-tree").unwrap();
    let config = KernelConfig::new([sessions])
        .unwrap()
        .with_layer_policy(
            session_service(),
            vec![LayerPolicy {
                plugin: tree.clone(),
                priority: 100,
                required: true,
                enabled: true,
            }],
        )
        .unwrap();

    assert_eq!(
        config
            .resolve_chain(&session_service(), &authority(), None)
            .unwrap_err(),
        KernelError::RequiredLayerUnavailable {
            service: session_service(),
            plugin: tree,
        }
    );
}
