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
                increase(&mut state, Increase { by: u64::MAX }).unwrap().value,
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

#[cfg(test)]
mod contract_tests {
    use super::*;
    use phenix_sdk::{ContractMetadata, Operation, Value};

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
