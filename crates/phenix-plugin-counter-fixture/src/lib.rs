//! Domain-owned ABI contract fixture. No graph-v1 compatibility declaration.
use phenix_sdk::Portable;

#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Increase {
    pub by: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct CounterValue {
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub enum CounterError {
    Overflow,
}

#[phenix_sdk::contract(id = "fixture.counter@1")]
pub trait Counter {
    fn increase(&mut self, change: Increase) -> Result<CounterValue, CounterError>;
    fn read(&self) -> Result<CounterValue, CounterError>;
}

#[phenix_sdk::plugin(
    id = "phenix.fixture.counter",
    version = "1.0.0",
    define(Counter),
    provide(Counter, slot = "Counter")
)]
mod plugin {
    use super::*;

    #[derive(Default)]
    pub struct State {
        value: u64,
    }

    #[provide(Counter::increase)]
    fn increase(state: &mut State, change: Increase) -> Result<CounterValue, CounterError> {
        let value = state
            .value
            .checked_add(change.by)
            .ok_or(CounterError::Overflow)?;
        state.value = value;
        Ok(CounterValue { value })
    }

    #[provide(Counter::read)]
    fn read(state: &mut State) -> CounterValue {
        CounterValue { value: state.value }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn state_is_private_and_failed_transition_is_atomic() {
            let mut state = State::default();
            assert_eq!(read(&mut state).value, 0);
            assert_eq!(
                increase(&mut state, Increase { by: u64::MAX })
                    .unwrap()
                    .value,
                u64::MAX
            );
            assert_eq!(
                increase(&mut state, Increase { by: 1 }),
                Err(CounterError::Overflow)
            );
            assert_eq!(read(&mut state).value, u64::MAX);
            let other = State::default();
            assert_eq!(other.value, 0);
        }
    }
}

/// Plugin-owned graph-v2 topology (offline V0; not yet an executable descriptor).
/// Uses exactly the same operation markers and Portable types as the ABI.
pub fn topology() -> Result<phenix_sdk::graph_v2::GraphContributionV2, phenix_sdk::Failure> {
    use phenix_sdk::graph::Graph;

    let mut graph = Graph::new("phenix.fixture.counter");
    let ingress = graph.module("ingress", 1, |module| {
        let submitted = module.source::<Increase>("submitted", "fixture.counter.submitted")?;
        module.exit("increase", submitted)
    })?;
    let counter = graph.module("counter", 1, |module| {
        let increase = module.invoke::<Counter::increase>("increase")?;
        let read = module.invoke::<Counter::read>("read")?;
        module.entry("increase", increase.input())?;
        module.entry("read", read.input())?;
        module.exit("count", increase.ok())?;
        module.exit("snapshot", read.ok())?;
        Ok(())
    })?;
    graph.flow(
        ingress.output::<Increase>("increase")?,
        counter.input::<Increase>("increase")?,
    );
    graph.finish()
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use phenix_sdk::{ContractMetadata, Operation, Value};

    #[test]
    fn module_topology_reuses_local_operation_and_type_signatures() {
        use phenix_sdk::graph_v2::NodeKindV2;

        let graph = topology().unwrap();
        assert_eq!(graph.modules.len(), 2);
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.lowered_flows().unwrap().len(), 1);
        assert_eq!(
            graph
                .nodes
                .iter()
                .filter(|node| matches!(node.kind, NodeKindV2::Invoke { .. }))
                .count(),
            2
        );
        assert!(graph.nodes.iter().any(|node| {
            matches!(
                &node.kind,
                NodeKindV2::Invoke { operation, .. }
                    if operation == Counter::increase::KEY
            )
        }));
        let counter = graph.modules.iter().find(|module| module.id == "counter").unwrap();
        assert!(counter.entries.iter().any(|port| port.name == "increase"));
        assert!(counter.exits.iter().any(|port| port.name == "count"));
    }

    #[test]
    fn local_signature_generates_abi_contract_metadata() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(metadata.plugin, "phenix.fixture.counter");
        assert!(metadata.requires.is_empty());
        assert_eq!(metadata.definitions.len(), 1);
        assert_eq!(metadata.definitions[0].contract, "fixture.counter@1");
        assert_eq!(metadata.provides.len(), 1);
        assert_eq!(metadata.provides[0].contract, "fixture.counter@1");
        assert_eq!(metadata.definitions[0].operations.len(), 2);
        let operation = &metadata.definitions[0].operations[0];
        assert_eq!(operation.name, "increase");
        assert_eq!(operation.request, Increase::SCHEMA.owned().unwrap());
        assert_eq!(operation.response, CounterValue::SCHEMA.owned().unwrap());
        assert_eq!(operation.error, CounterError::SCHEMA.owned().unwrap());
        assert_eq!(Counter::increase::KEY, "fixture.counter@1/increase");
        let original = Increase { by: 17 };
        let encoded = original.to_value().encode().unwrap();
        assert_eq!(
            Increase::from_value(Value::decode(&encoded).unwrap()).unwrap(),
            original
        );
    }
}
