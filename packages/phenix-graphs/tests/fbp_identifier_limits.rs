use phenix_graph::{
    CompileLimit, CompileLimits, ComponentDef, Contract, Interface, LimitKind,
    MAX_IDENTIFIER_BYTES, NetworkBuilder, NetworkError, PortDef,
    process::{Process, ProcessContext, Step},
};
use std::sync::Arc;

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        Ok(Step::Finished)
    }
}

#[test]
fn compile_limits_retain_kind_configuration_exact_attempt_and_path() {
    use std::error::Error;
    let mut child = NetworkBuilder::new();
    child
        .module("leaf", NetworkBuilder::new().build().unwrap())
        .unwrap();
    let mut root = NetworkBuilder::new();
    root.module("module", child.build().unwrap()).unwrap();
    let network = root.build().unwrap();
    for (limits, kind, configured, attempted, path) in [
        (
            CompileLimits {
                max_work: 0,
                ..CompileLimits::default()
            },
            LimitKind::Work,
            0,
            1,
            "",
        ),
        (
            CompileLimits {
                max_depth: 0,
                ..CompileLimits::default()
            },
            LimitKind::Depth,
            0,
            1,
            "module",
        ),
        (
            CompileLimits {
                max_depth: 1,
                ..CompileLimits::default()
            },
            LimitKind::Depth,
            1,
            2,
            "module/leaf",
        ),
        (
            CompileLimits {
                max_instances: 0,
                ..CompileLimits::default()
            },
            LimitKind::Instances,
            0,
            1,
            "module",
        ),
        (
            CompileLimits {
                max_instances: 1,
                ..CompileLimits::default()
            },
            LimitKind::Instances,
            1,
            2,
            "module/leaf",
        ),
    ] {
        let error = network.compile_with_limits(limits).unwrap_err();
        assert_eq!(
            error,
            NetworkError::LimitExceeded {
                limit: CompileLimit {
                    kind,
                    configured,
                    attempted: Some(attempted)
                },
                path: path.into(),
            }
        );
        let failure = phenix_graph::Failure::external("compile limit exceeded", error.clone());
        let source = failure
            .source()
            .unwrap()
            .downcast_ref::<NetworkError>()
            .unwrap();
        assert_eq!(source, &error);
        assert!(source.source().is_none());
    }
    network
        .compile_with_limits(CompileLimits {
            max_depth: 2,
            max_instances: 2,
            max_work: 2,
        })
        .unwrap();
    assert_eq!(
        network
            .compile_with_limits(CompileLimits {
                max_work: 1,
                ..CompileLimits::default()
            })
            .unwrap_err(),
        NetworkError::LimitExceeded {
            limit: CompileLimit {
                kind: LimitKind::Work,
                configured: 1,
                attempted: Some(2)
            },
            path: "module".into(),
        }
    );
    // The root and module paths name the same public types, not wrapper errors.
    let _: phenix_graph::network::CompileLimit = CompileLimit {
        kind: phenix_graph::network::LimitKind::Work,
        configured: 0,
        attempted: Some(1),
    };
}

#[test]
fn composition_origin_scan_reports_its_existing_depth_and_instance_bounds() {
    use phenix_graph::{ComposeError, Composer, Edit, GraphOrigin, NetworkPatch, Registration};
    let limits = CompileLimits::default();
    let mut deep = ComponentDef::composite(NetworkBuilder::new().build().unwrap());
    for _ in 0..limits.max_depth {
        let mut builder = NetworkBuilder::new();
        builder.add("child", Arc::new(deep)).unwrap();
        deep = ComponentDef::composite(builder.build().unwrap());
    }
    let mut wide = NetworkBuilder::new();
    let leaf = Arc::new(ComponentDef::composite(
        NetworkBuilder::new().build().unwrap(),
    ));
    for i in 0..limits.max_instances {
        wide.add(format!("child{i}"), leaf.clone()).unwrap();
    }
    for (definition, kind, configured, attempted, path) in [
        (
            deep,
            LimitKind::Depth,
            limits.max_depth as u64,
            limits.max_depth as u64 + 1,
            format!("root{}", "/child".repeat(limits.max_depth)),
        ),
        (
            ComponentDef::composite(wide.build().unwrap()),
            LimitKind::Instances,
            limits.max_instances,
            limits.max_instances + 1,
            "root".into(),
        ),
    ] {
        let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
        let patch = NetworkPatch::new([Edit::Add {
            path: "root".into(),
            definition: Arc::new(definition),
        }]);
        assert_eq!(
            composer
                .register([Registration::new(
                    "bounded",
                    [],
                    0,
                    GraphOrigin::default(),
                    patch.footprints(),
                    patch
                )])
                .unwrap_err(),
            ComposeError::Network(NetworkError::LimitExceeded {
                limit: CompileLimit {
                    kind,
                    configured,
                    attempted: Some(attempted)
                },
                path,
            })
        );
        assert!(composer.current().network().instances().is_empty());
        assert!(composer.current().compiled().processes().is_empty());
        assert!(composer.revoke("bounded").unwrap().is_empty());
    }
}

#[test]
fn identifier_limit_accepts_exact_boundary_and_rejects_next_byte() {
    let boundary = "x".repeat(MAX_IDENTIFIER_BYTES);
    let oversized = "x".repeat(MAX_IDENTIFIER_BYTES + 1);

    assert!(Interface::new([PortDef::local::<i64>(boundary.clone())], []).is_ok());
    assert!(Interface::new([PortDef::local::<i64>(oversized.clone())], []).is_err());
    let contract_boundary = format!("{}/{}", "x".repeat(255), "x".repeat(256));
    let contract_oversized = format!("{}/{}", "x".repeat(256), "x".repeat(256));
    assert_eq!(contract_boundary.len(), MAX_IDENTIFIER_BYTES);
    assert!(Contract::new(contract_boundary, Arc::new(Interface::default())).is_ok());
    assert!(Contract::new(contract_oversized, Arc::new(Interface::default())).is_err());

    let mut builder = NetworkBuilder::new();
    let interface = Arc::new(Interface::default());
    assert!(
        builder
            .node(boundary, interface.clone(), Arc::new(|| Noop))
            .is_ok()
    );
    assert!(
        builder
            .node(oversized, interface, Arc::new(|| Noop))
            .is_err()
    );
}
