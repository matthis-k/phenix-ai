use crate::{
    CallableRef, PhenixValue, ReferenceGenerationId, ReferenceId, ReferenceOwnerId, Type,
    ValueError,
};
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use thiserror::Error;

/// The transport-neutral input to a callable invocation.
#[derive(Clone, Debug, PartialEq)]
pub struct CallableInvocation {
    pub callable: CallableRef,
    pub input: PhenixValue,
}

/// The transport-neutral successful result of a callable invocation.
#[derive(Clone, Debug, PartialEq)]
pub struct CallableInvocationResult {
    pub output: PhenixValue,
}

impl CallableInvocation {
    /// Validates the reference contract and input before handler code may run.
    pub fn validate(&self, callable_schema: &Type) -> Result<(), ValueError> {
        let Type::Callable {
            contract, input, ..
        } = callable_schema
        else {
            return Err(ValueError::InvalidValue(
                "callable invocation requires a callable schema".to_owned(),
            ));
        };
        if self.callable.contract() != contract {
            return Err(ValueError::InvalidValue(format!(
                "callable reference contract {} does not match schema contract {contract}",
                self.callable.contract()
            )));
        }
        input.parse(&self.input)
    }
}

impl CallableInvocationResult {
    /// Validates handler output before it crosses the callable boundary.
    pub fn validate(&self, callable_schema: &Type) -> Result<(), ValueError> {
        let Type::Callable { output, .. } = callable_schema else {
            return Err(ValueError::InvalidValue(
                "callable invocation requires a callable schema".to_owned(),
            ));
        };
        output.parse(&self.output)
    }
}

/// The owner-side implementation of a callable.
///
/// The registry validates both sides of every call. Handlers therefore receive
/// only values that satisfy their declared input schema and cannot publish an
/// unchecked output.
pub trait CallableHandler: Send + Sync {
    fn invoke(&self, input: PhenixValue) -> Result<PhenixValue, CallableError>;
}

impl<F> CallableHandler for F
where
    F: Fn(PhenixValue) -> Result<PhenixValue, CallableError> + Send + Sync,
{
    fn invoke(&self, input: PhenixValue) -> Result<PhenixValue, CallableError> {
        self(input)
    }
}

#[derive(Clone)]
struct RegisteredCallable {
    schema: Type,
    handler: Arc<dyn CallableHandler>,
}

type CallableKey = (ReferenceOwnerId, ReferenceGenerationId, ReferenceId);

/// Process-local callable dispatch keyed by opaque owner, generation, and
/// reference identities. Transport adapters retain ownership of client queues;
/// they register a handler that forwards the canonical invocation unchanged.
#[derive(Default)]
pub struct CallableRegistry {
    entries: BTreeMap<CallableKey, RegisteredCallable>,
    retired: BTreeSet<(ReferenceOwnerId, ReferenceGenerationId)>,
}

/// A synchronization wrapper for dispatch that permits a callable handler to
/// invoke another callable. The registry lock is released before provider
/// code runs, so callbacks can safely re-enter through the same dispatcher.
#[derive(Clone, Default)]
pub struct SharedCallableRegistry(Arc<Mutex<CallableRegistry>>);

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CallableError {
    #[error("unknown callable reference {}", .0.id())]
    UnknownReference(CallableRef),
    #[error("stale callable reference {}", .0.id())]
    StaleReference(CallableRef),
    #[error("duplicate callable reference {}", .0.id())]
    DuplicateReference(CallableRef),
    #[error("callable schema mismatch: {message}")]
    SchemaMismatch { message: String },
    #[error("callable handler failed: {message}")]
    HandlerFailed { message: String },
    #[error("callable invocation cancelled")]
    Cancelled,
    #[error("callable handler disconnected")]
    Disconnected,
    #[error("callable handler queue is full")]
    QueueFull,
}

impl CallableRegistry {
    /// Registers one callable for its exact owner generation.
    pub fn register(
        &mut self,
        reference: CallableRef,
        schema: Type,
        handler: impl CallableHandler + 'static,
    ) -> Result<(), CallableError> {
        let Type::Callable { contract, .. } = &schema else {
            return Err(CallableError::SchemaMismatch {
                message: "registered callable schema is not callable".to_owned(),
            });
        };
        if reference.contract() != contract {
            return Err(CallableError::SchemaMismatch {
                message: format!(
                    "reference contract {} does not match schema contract {contract}",
                    reference.contract()
                ),
            });
        }
        let key = (
            reference.owner().clone(),
            reference.generation().clone(),
            reference.id().clone(),
        );
        if self.retired.contains(&(key.0.clone(), key.1.clone())) {
            return Err(CallableError::StaleReference(reference));
        }
        if self.entries.contains_key(&key) {
            return Err(CallableError::DuplicateReference(reference));
        }
        self.entries.insert(
            key,
            RegisteredCallable {
                schema,
                handler: Arc::new(handler),
            },
        );
        Ok(())
    }

    /// Makes every reference from an owner generation permanently stale.
    pub fn retire(&mut self, owner: ReferenceOwnerId, generation: ReferenceGenerationId) {
        self.entries
            .retain(|(entry_owner, entry_generation, _), _| {
                entry_owner != &owner || entry_generation != &generation
            });
        self.retired.insert((owner, generation));
    }

    /// Removes one live callable without affecting other references from the
    /// same owner generation. This is used for explicit stop handles and
    /// session-scoped admissions whose owner remains connected.
    pub fn unregister(&mut self, reference: &CallableRef) -> bool {
        if self
            .retired
            .contains(&(reference.owner().clone(), reference.generation().clone()))
        {
            return false;
        }
        self.entries
            .remove(&(
                reference.owner().clone(),
                reference.generation().clone(),
                reference.id().clone(),
            ))
            .is_some()
    }

    /// Invokes one reference after input validation and before output release.
    pub fn invoke(
        &self,
        invocation: CallableInvocation,
    ) -> Result<CallableInvocationResult, CallableError> {
        self.prepare(invocation)?.invoke()
    }

    fn prepare(
        &self,
        invocation: CallableInvocation,
    ) -> Result<PreparedCallableInvocation, CallableError> {
        let reference = &invocation.callable;
        let owner_generation = (reference.owner().clone(), reference.generation().clone());
        if self.retired.contains(&owner_generation) {
            return Err(CallableError::StaleReference(reference.clone()));
        }
        let key = (
            reference.owner().clone(),
            reference.generation().clone(),
            reference.id().clone(),
        );
        let entry = self
            .entries
            .get(&key)
            .ok_or_else(|| CallableError::UnknownReference(reference.clone()))?;
        invocation
            .validate(&entry.schema)
            .map_err(|error| CallableError::SchemaMismatch {
                message: error.to_string(),
            })?;
        Ok(PreparedCallableInvocation {
            schema: entry.schema.clone(),
            handler: Arc::clone(&entry.handler),
            input: invocation.input,
        })
    }

    fn schema(&self, reference: &CallableRef) -> Result<Type, CallableError> {
        let owner_generation = (reference.owner().clone(), reference.generation().clone());
        if self.retired.contains(&owner_generation) {
            return Err(CallableError::StaleReference(reference.clone()));
        }
        self.entries
            .get(&(
                reference.owner().clone(),
                reference.generation().clone(),
                reference.id().clone(),
            ))
            .map(|entry| entry.schema.clone())
            .ok_or_else(|| CallableError::UnknownReference(reference.clone()))
    }
}

struct PreparedCallableInvocation {
    schema: Type,
    handler: Arc<dyn CallableHandler>,
    input: PhenixValue,
}

impl PreparedCallableInvocation {
    fn invoke(self) -> Result<CallableInvocationResult, CallableError> {
        let result = CallableInvocationResult {
            output: self.handler.invoke(self.input)?,
        };
        result
            .validate(&self.schema)
            .map_err(|error| CallableError::SchemaMismatch {
                message: error.to_string(),
            })?;
        Ok(result)
    }
}

impl SharedCallableRegistry {
    pub fn register(
        &self,
        reference: CallableRef,
        schema: Type,
        handler: impl CallableHandler + 'static,
    ) -> Result<(), CallableError> {
        self.0.lock().register(reference, schema, handler)
    }

    pub fn retire(&self, owner: ReferenceOwnerId, generation: ReferenceGenerationId) {
        self.0.lock().retire(owner, generation);
    }

    pub fn unregister(&self, reference: &CallableRef) -> bool {
        self.0.lock().unregister(reference)
    }

    pub fn invoke(
        &self,
        invocation: CallableInvocation,
    ) -> Result<CallableInvocationResult, CallableError> {
        let prepared = self.0.lock().prepare(invocation)?;
        prepared.invoke()
    }

    /// Returns the authoritative callable schema for one live reference.
    pub fn schema(&self, reference: &CallableRef) -> Result<Type, CallableError> {
        self.0.lock().schema(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ClientConnectionId, ContractId, ReferenceGenerationId, ReferenceId, ReferenceOwnerId,
    };

    fn schema() -> Type {
        Type::Callable {
            contract: ContractId::parse("fixture.callback@1").unwrap(),
            input: Box::new(Type::U64),
            output: Box::new(Type::String),
        }
    }

    fn reference() -> CallableRef {
        CallableRef::new(
            ContractId::parse("fixture.callback@1").unwrap(),
            ReferenceOwnerId::Client(ClientConnectionId::parse("fixture.client").unwrap()),
            ReferenceGenerationId::parse("fixture.generation").unwrap(),
            ReferenceId::parse("fixture.callback").unwrap(),
        )
    }

    #[test]
    fn invocation_validates_contract_and_input_before_dispatch() {
        let invocation = CallableInvocation {
            callable: reference(),
            input: PhenixValue::U64(1),
        };
        invocation.validate(&schema()).unwrap();

        let wrong_input = CallableInvocation {
            callable: reference(),
            input: PhenixValue::String("wrong".to_owned()),
        };
        assert!(wrong_input.validate(&schema()).is_err());
    }

    #[test]
    fn invocation_validates_output_before_returning_to_the_caller() {
        CallableInvocationResult {
            output: PhenixValue::String("ok".to_owned()),
        }
        .validate(&schema())
        .unwrap();
        assert!(CallableInvocationResult {
            output: PhenixValue::U64(1),
        }
        .validate(&schema())
        .is_err());
    }

    #[test]
    fn registry_validates_both_sides_of_a_plugin_or_client_capability_call() {
        let reference = reference();
        let mut registry = CallableRegistry::default();
        registry
            .register(reference.clone(), schema(), |input| match input {
                PhenixValue::U64(value) => Ok(PhenixValue::String(value.to_string())),
                _ => unreachable!("registry validates input before handler execution"),
            })
            .unwrap();

        let result = registry
            .invoke(CallableInvocation {
                callable: reference.clone(),
                input: PhenixValue::U64(7),
            })
            .unwrap();
        assert_eq!(result.output, PhenixValue::String("7".to_owned()));

        let error = registry
            .invoke(CallableInvocation {
                callable: reference,
                input: PhenixValue::String("wrong".to_owned()),
            })
            .unwrap_err();
        assert!(matches!(error, CallableError::SchemaMismatch { .. }));
    }

    #[test]
    fn retired_owner_generation_is_structurally_stale() {
        let reference = reference();
        let mut registry = CallableRegistry::default();
        registry
            .register(reference.clone(), schema(), |_| {
                Ok(PhenixValue::String("ok".to_owned()))
            })
            .unwrap();
        registry.retire(reference.owner().clone(), reference.generation().clone());

        assert_eq!(
            registry
                .invoke(CallableInvocation {
                    callable: reference.clone(),
                    input: PhenixValue::U64(1),
                })
                .unwrap_err(),
            CallableError::StaleReference(reference)
        );
    }

    #[test]
    fn unregister_removes_one_capability_without_retiring_its_generation() {
        let reference = reference();
        let retained = CallableRef::new(
            ContractId::parse("fixture.callback@1").unwrap(),
            reference.owner().clone(),
            reference.generation().clone(),
            ReferenceId::parse("fixture.retained").unwrap(),
        );
        let mut registry = CallableRegistry::default();
        for callable in [&reference, &retained] {
            registry
                .register(callable.clone(), schema(), |_| {
                    Ok(PhenixValue::String("ok".to_owned()))
                })
                .unwrap();
        }

        assert!(registry.unregister(&reference));
        assert!(!registry.unregister(&reference));
        assert!(matches!(
            registry.invoke(CallableInvocation {
                callable: reference,
                input: PhenixValue::U64(1),
            }),
            Err(CallableError::UnknownReference(_))
        ));
        assert_eq!(
            registry
                .invoke(CallableInvocation {
                    callable: retained,
                    input: PhenixValue::U64(1),
                })
                .unwrap()
                .output,
            PhenixValue::String("ok".to_owned())
        );
    }

    #[test]
    fn invalid_provider_output_never_reaches_the_caller() {
        let reference = reference();
        let mut registry = CallableRegistry::default();
        registry
            .register(reference.clone(), schema(), |_| Ok(PhenixValue::U64(1)))
            .unwrap();

        assert!(matches!(
            registry.invoke(CallableInvocation {
                callable: reference,
                input: PhenixValue::U64(1),
            }),
            Err(CallableError::SchemaMismatch { .. })
        ));
    }
}
