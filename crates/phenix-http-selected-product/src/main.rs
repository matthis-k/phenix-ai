//! Isolated C3 product gate: private native Model installation and real HTTP.
use phenix_basic_contracts::{
    BasicError, ErrorCode, EventPage, JournalQuery, Limits, Message, ModelReply, ModelRequest,
};
use phenix_contract::{Authority, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Call, Outcome, Poll,
};
use phenix_provider_contracts::{
    bind_http_model, validate_model_selection, AdapterBinding, ApiKeyOptions, ApiKeyPresentation, AuthenticationKind,
    AuthenticationMethod, AuthenticationStatus, ConnectionDescriptor, CredentialIdentity,
    CredentialProfile, CredentialProfileStatus, CredentialSecret, HttpModelInstall, ModelOrigin,
    ModelPage, ModelSelection, ProtocolBinding, ProviderDescriptor, ProviderError,
    ProviderErrorCode, ProviderModel,
};
use std::{
    collections::BTreeSet,
    fs,
    time::{Duration, Instant},
};

fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}

fn checked_install(endpoint: &str, session: &str, authenticated: bool) -> HttpModelInstall {
    // The fixture is deterministic test data, not an authoritative catalog.
    // Production controller admission must read current provider-owned state.
    let provider = ProviderDescriptor {
        id: "local-fixture".into(),
        label: "Local fixture".into(),
        connections: vec![ConnectionDescriptor {
            id: "local".into(),
            label: "Local".into(),
            protocols: vec![ProtocolBinding {
                id: "openai-chat".into(),
                endpoint: Some(endpoint.into()),
            }],
            base_url: None,
            auth_methods: if authenticated {
                vec![AuthenticationMethod {
                    id: "api-key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
                    adapter: AdapterBinding::from_typed(
                        "api-key",
                        &ApiKeyOptions {
                            environment_variable: None,
                            interactive: true,
                            presentation: ApiKeyPresentation::Bearer,
                        },
                    )
                    .unwrap(),
                }]
            } else {
                vec![]
            },
            model_sources: vec![],
        }],
    };
    let auth = AuthenticationStatus {
        provider_id: provider.id.clone(),
        connection_id: "local".into(),
        profiles: if authenticated {
            vec![CredentialProfile {
                id: "primary".into(),
                label: "Primary".into(),
                method_id: "api-key".into(),
                status: CredentialProfileStatus::Authenticated,
            }]
        } else {
            vec![]
        },
    };
    let profile_id = authenticated.then(|| "primary".to_owned());
    let models = ModelPage {
        profile_id: profile_id.clone(),
        models: vec![ProviderModel {
            id: "selected-fixture".into(),
            label: "Selected fixture".into(),
            connection_id: "local".into(),
            protocol_id: "openai-chat".into(),
            endpoint_id: "chat".into(),
            capabilities: vec![],
            origin: ModelOrigin::Discovered,
            selectable: true,
        }],
        next_cursor: None,
        revision: 1,
    };
    let request = ModelSelection {
        provider_id: provider.id.clone(),
        connection_id: "local".into(),
        profile_id,
        model_id: "selected-fixture".into(),
        protocol_id: "openai-chat".into(),
        endpoint_id: "chat".into(),
        catalog_revision: 1,
    };
    let chosen = validate_model_selection(&provider, &auth, &models, &request, true).unwrap();
    let bound = bind_http_model(&provider, chosen).unwrap();
    if authenticated {
        let correct = CredentialIdentity::new("local-fixture", "local", "primary").unwrap();
        let foreign = CredentialIdentity::new("local-fixture", "local", "foreign").unwrap();
        let new_secret = || CredentialSecret::new(b"selected-fixture-secret".to_vec()).unwrap();
        // Admission and secret identity are distinct: stale/foreign or absent
        // API-key profiles must fail before invoking any provider I/O.
        assert_eq!(
            HttpModelInstall::from_checked(
                &bound,
                session.into(),
                Some((foreign, new_secret())),
                None
            )
            .unwrap_err()
            .code,
            ProviderErrorCode::Denied
        );
        assert_eq!(
            HttpModelInstall::from_checked(&bound, session.into(), None, None)
                .unwrap_err()
                .code,
            ProviderErrorCode::Denied
        );
        HttpModelInstall::from_checked(&bound, session.into(), Some((correct, new_secret())), None)
            .unwrap()
    } else {
        HttpModelInstall::from_checked(&bound, session.into(), None, None).unwrap()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(
        args.len(),
        7,
        "HTTP_PLUGIN JOURNAL SQLITE BASIC_DEFINITIONS PROVIDER_DEFINITIONS OUTPUT"
    );
    let endpoint =
        std::env::var("PHENIX_SELECTED_HTTP_ENDPOINT").expect("selected loopback endpoint");
    let authenticated = match std::env::var("PHENIX_SELECTED_HTTP_AUTH_MODE").as_deref() {
        Ok("api-key") => true,
        Ok("none") => false,
        _ => panic!("explicit credential-free or API-key fixture mode is required"),
    };
    let resource = BTreeSet::from(["phenix.session.s1".to_owned()]);
    let specs = [
        ("root", "phenix.provider.http-model-session@1/install"),
        ("root", "phenix.basic.model@1/complete"),
        ("root", "phenix.basic.journal@1/read"),
        ("phenix.model.http", "phenix.basic.journal@1/append"),
        ("phenix.basic.event-journal", "phenix.basic.storage@1/read"),
        (
            "phenix.basic.event-journal",
            "phenix.basic.storage@1/transact",
        ),
    ];
    let kernel = Prepared::prepare(Composition {
        format: "phenix.composition.v3".into(),
        artifacts: args[1..4].to_vec(),
        definitions: args[4..6].to_vec(),
        entry: selection("phenix.basic.model@1/complete", "phenix.model.http"),
        slots: vec![
            SlotBinding {
                caller: "phenix.model.http".into(),
                slot: "Journal".into(),
                index: 0,
                provider: "phenix.basic.event-journal".into(),
                export: "Journal".into(),
            },
            SlotBinding {
                caller: "phenix.basic.event-journal".into(),
                slot: "Storage".into(),
                index: 0,
                provider: "phenix.basic.storage-sqlite".into(),
                export: "Storage".into(),
            },
        ],
        grants: specs
            .iter()
            .map(|(caller, key)| {
                let (interface, operation) = key.split_once('/').unwrap();
                ScopedGrant {
                    caller: (*caller).into(),
                    interface: interface.into(),
                    operation: operation.into(),
                    resources: resource.clone(),
                }
            })
            .collect(),
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: specs.iter().map(|(_, key)| (*key).into()).collect(),
            resources: resource,
        },
        resource: Some("phenix.session.s1".into()),
    })
    .unwrap()
    .load()
    .unwrap();
    let root = kernel.admit_default_root().unwrap();
    let control = selection(
        "phenix.provider.http-model-session@1/install",
        "phenix.model.http",
    );

    // A request for s2 cannot use this s1-only root even when the
    // metadata passes the ordinary provider catalog checks.
    let foreign = checked_install(&endpoint, "s2", authenticated);
    match root.invoke_selected(&control, &foreign.to_value()).unwrap() {
        Outcome::Domain(value) => {
            assert_eq!(
                ProviderError::from_value(value).unwrap().code,
                ProviderErrorCode::Denied
            );
        }
        Outcome::Success(_) => panic!("cross-session native model install was accepted"),
    }

    let chosen = checked_install(&endpoint, "s1", authenticated);
    assert!(matches!(
        root.invoke_selected(&control, &chosen.to_value()).unwrap(),
        Outcome::Success(_)
    ));
    let request = ModelRequest {
        session: "s1".into(),
        run: "r1".into(),
        history: vec![Message::user("selected")],
        tools: vec![],
        skills: vec![],
        context: vec![],
        limits: Limits {
            model_tokens: Some(32),
            ..Limits::default()
        },
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let ticket = match root.begin(&request.to_value()).unwrap() {
        Call::Pending(ticket) => ticket,
        Call::Ready(_) => panic!("selected live HTTP did not start a pending model"),
    };
    let reply = loop {
        assert!(Instant::now() < deadline, "selected native HTTP deadline");
        match root.poll(&ticket).unwrap() {
            Poll::Ready(value) => {
                let result = match value.unwrap() {
                    Outcome::Success(value) => ModelReply::from_value(value).unwrap(),
                    Outcome::Domain(value) => {
                        panic!("selected model denied: {:?}", BasicError::from_value(value));
                    }
                };
                root.release(&ticket).unwrap();
                break result;
            }
            Poll::Pending => std::thread::sleep(Duration::from_millis(5)),
            _ => panic!("selected native model unexpectedly cancelled"),
        }
    };
    assert_eq!(reply.text, "selected-live");
    assert_eq!(
        (reply.usage.input_tokens, reply.usage.output_tokens),
        (27, 7)
    );
    assert!(reply.calls.is_empty());
    let journal = root
        .invoke_selected(
            &selection("phenix.basic.journal@1/read", "phenix.basic.event-journal"),
            &JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 128,
            }
            .to_value(),
        )
        .unwrap();
    let page = match journal {
        Outcome::Success(value) => EventPage::from_value(value).unwrap(),
        Outcome::Domain(_) => panic!("journal read failed"),
    };
    assert_eq!(
        page.events
            .iter()
            .map(|event| event.text.as_str())
            .collect::<String>(),
        reply.text
    );
    // This grant is spent. An unrelated PHENIX_MODEL_HTTP_* environment
    // preset must never take over the next selected Model turn.
    let second = root.begin(&request.to_value()).unwrap();
    let rejected = match second {
        Call::Ready(value) => value,
        Call::Pending(ticket) => {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                assert!(Instant::now() < deadline, "second turn denial deadline");
                match root.poll(&ticket).unwrap() {
                    Poll::Ready(value) => {
                        root.release(&ticket).unwrap();
                        break value.unwrap();
                    }
                    Poll::Pending => std::thread::sleep(Duration::from_millis(5)),
                    _ => panic!("second turn unexpectedly cancelled"),
                }
            }
        }
    };
    match rejected {
        Outcome::Domain(value) => {
            assert_eq!(
                BasicError::from_value(value).unwrap().code,
                ErrorCode::InvalidInput
            );
        }
        Outcome::Success(_) => panic!("consumed native choice unexpectedly executed again"),
    }
    assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
    drop(root);
    kernel.shutdown().unwrap();
    fs::create_dir_all(&args[6]).unwrap();
    fs::write(
        format!("{}/accepted.txt", args[6]),
        "selected-native-http\n",
    )
    .unwrap();
}
