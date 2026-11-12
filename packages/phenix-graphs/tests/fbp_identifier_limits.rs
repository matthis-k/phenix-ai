use phenix_graph::{
    Contract, Interface, MAX_IDENTIFIER_BYTES, NetworkBuilder, PortDef,
    process::{Process, ProcessContext, Step},
};
use std::sync::Arc;

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
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
