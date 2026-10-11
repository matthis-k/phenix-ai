//! Authenticated state effects and ABI 3 imports enter the existing runtime.
use crate::{
    resolve::Selection,
    runtime::{authenticate_scope, Caller, Kernel},
    Call, Outcome,
};
use phenix_contract::{
    abi::{Bytes, Reply},
    Code, CompareExchange, Failure, StateRequest, Value, ValueAccessError as Error, ValueLocation,
    WireProfile,
};

impl Kernel {
    fn authorize_state(
        &self,
        caller: &str,
        location: &ValueLocation,
        operation: &str,
    ) -> Result<(), Error> {
        let cells = self.cells.borrow();
        let (owner, property) = cells.identity(location)?;
        let enabled = match operation {
            "get" => property.get,
            "set" => property.set,
            "watch" => property.watch,
            _ => false,
        };
        let interface =
            phenix_artifact::property_contract(owner, &property.id).map_err(|_| Error::Denied)?;
        let resource = interface.strip_suffix("@1").unwrap();
        let key = format!("{interface}/{operation}");
        let selected = caller == "root"
            || caller == owner
            || self.prepared.packages[caller]
                .descriptor()
                .imports
                .iter()
                .any(|i| {
                    i.operation == key
                        && self
                            .prepared
                            .bindings
                            .get(&(caller.into(), i.binding_key()))
                            .is_some_and(|p| p == owner)
                });
        if !enabled || !selected {
            let _ = self.fail::<()>(Code::Denied, "state access is not selected");
            return Err(Error::Denied);
        }
        let scope = self
            .effect_scope(caller, owner, &key, None)
            .map_err(|_| Error::Denied)?;
        if !scope.authority.resources.contains(resource) {
            let _ = self.fail::<()>(Code::Denied, "state property resource is not granted");
            return Err(Error::Denied);
        }
        Ok(())
    }
    pub(super) fn state_request(
        &self,
        root: u64,
        caller: &str,
        request: StateRequest,
    ) -> Result<Result<Value, Error>, Failure> {
        if let Some(owner) = self.initializing.borrow().as_ref() {
            if owner != caller {
                return Err(Failure::new(
                    Code::ScopeRejected,
                    "foreign initialization caller",
                ));
            }
            let StateRequest::Create { property, initial } = request else {
                return Err(Failure::new(
                    Code::ScopeRejected,
                    "only property creation is allowed during initialization",
                ));
            };
            let property = self.prepared.packages[caller]
                .metadata()
                .unwrap()
                .properties
                .iter()
                .find(|p| p.id == property)
                .ok_or_else(|| Failure::new(Code::Denied, "undeclared initialization property"))?;
            let location = self.cells.borrow_mut().create(caller, property, initial)?;
            return Ok(Ok(Value::Ref(Box::new(location))));
        }
        if self.state.borrow().active_root != Some(root) {
            return Err(Failure::new(
                Code::ScopeRejected,
                "state callback has no live root",
            ));
        }
        let ticket = self.state.borrow().stack.last().and_then(|f| f.ticket);
        let result = (|| match request {
            StateRequest::Create { .. } => Err(Error::Denied),
            StateRequest::Read(r) => {
                self.authorize_state(caller, &r, "get")?;
                self.cells.borrow().read(&r)
            }
            StateRequest::CompareExchange(edit) => {
                self.authorize_state(caller, &edit.location, "set")?;
                let Value::List(mut batch) = self.cells.borrow_mut().commit(&[edit])? else {
                    unreachable!()
                };
                let Value::List(mut snapshots) = batch.pop().unwrap() else {
                    unreachable!()
                };
                Ok(snapshots.pop().unwrap())
            }
            StateRequest::Batch(edits) => {
                for edit in &edits {
                    self.authorize_state(caller, &edit.location, "set")?;
                }
                self.cells.borrow_mut().commit(&edits)
            }
            StateRequest::Subscribe(r) => {
                self.authorize_state(caller, &r, "watch")?;
                self.cells.borrow_mut().subscribe(root, caller, ticket, r)
            }
            StateRequest::Next(id) | StateRequest::Unsubscribe(id) => {
                let r = self.cells.borrow().cursor(root, caller, ticket, id)?;
                self.authorize_state(caller, &r, "watch")?;
                if matches!(request, StateRequest::Next(_)) {
                    self.cells.borrow_mut().next(root, caller, ticket, id)
                } else {
                    self.cells
                        .borrow_mut()
                        .unsubscribe(root, caller, ticket, id)
                }
            }
        })();
        Ok(result)
    }
    pub(super) fn property_dispatch(
        &self,
        root: u64,
        caller: &str,
        selection: &Selection,
        request: &Value,
    ) -> Result<Option<Outcome>, Failure> {
        let Some(metadata) = self.prepared.packages[&selection.provider].metadata() else {
            return Ok(None);
        };
        if !metadata.properties.iter().any(|p| {
            phenix_artifact::property_contract(&metadata.plugin, &p.id)
                .is_ok_and(|id| id == selection.interface)
        }) {
            return Ok(None);
        }
        let state_request = match (selection.operation.as_str(), request) {
            ("get", Value::Ref(r)) => StateRequest::Read(**r),
            ("watch", Value::Ref(r)) => StateRequest::Subscribe(**r),
            ("set", Value::Map(m)) => {
                let (Some(Value::Ref(r)), Some(Value::U64(expected)), Some(value)) =
                    (m.get("location"), m.get("expected"), m.get("value"))
                else {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        "invalid property update",
                    ));
                };
                StateRequest::CompareExchange(CompareExchange {
                    location: **r,
                    expected: phenix_contract::Revision(*expected),
                    value: value.clone(),
                })
            }
            _ => {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "invalid property request",
                ))
            }
        };
        // A property export can access only the selected owner's declared cell.
        let r = match &state_request {
            StateRequest::Read(r) | StateRequest::Subscribe(r) => r,
            StateRequest::CompareExchange(e) => &e.location,
            _ => unreachable!(),
        };
        if !self.cells.borrow().identity(r).is_ok_and(|(owner, p)| {
            owner == selection.provider
                && phenix_artifact::property_contract(owner, &p.id)
                    .is_ok_and(|id| id == selection.interface)
        }) {
            return Ok(Some(Outcome::Domain(Value::String(
                "StaleReference".into(),
            ))));
        }
        Ok(Some(
            match self.state_request(root, caller, state_request)? {
                Ok(Value::List(v)) => Outcome::Success(match selection.operation.as_str() {
                    "get" => v[2].clone(),
                    _ => v[0].clone(),
                }),
                Ok(_) => {
                    return Err(Failure::new(
                        Code::InfrastructureFailure,
                        "invalid property state response",
                    ))
                }
                Err(error) => Outcome::Domain(Value::String(
                    match error {
                        Error::Conflict { .. } => "Conflict",
                        Error::Denied => "Denied",
                        Error::OwnerGone => "OwnerGone",
                        Error::SchemaMismatch => "SchemaMismatch",
                        _ => "StaleReference",
                    }
                    .into(),
                )),
            },
        ))
    }
}
fn protected(body: impl FnOnce() -> Result<Reply, Failure>) -> Reply {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(Ok(reply)) => reply,
        Ok(Err(failure)) => Reply::state_value(2, failure.to_value()),
        Err(_) => Reply::state_value(
            2,
            Failure::new(Code::InfrastructureFailure, "caught state host panic").to_value(),
        ),
    }
}
pub(super) unsafe extern "C" fn state(scope: u64, opcode: u32, bytes: Bytes) -> Reply {
    protected(|| {
        let record = authenticate_scope(scope)?;
        let kernel = unsafe { &*(record.kernel as *const Kernel) };
        let request = StateRequest::decode(opcode, unsafe { bytes.read() }?)?;
        Ok(
            match kernel.state_request(record.root, &record.caller, request)? {
                Ok(value) => Reply::state_value(0, value),
                Err(error) => Reply::state_value(1, error.to_value()),
            },
        )
    })
}
pub(super) unsafe extern "C" fn invoke(scope: u64, slot: Bytes, key: Bytes, input: Bytes) -> Reply {
    protected(|| {
        let record = authenticate_scope(scope)?;
        let kernel = unsafe { &*(record.kernel as *const Kernel) };
        if kernel.initializing.borrow().is_some() {
            return Err(Failure::new(
                Code::ScopeRejected,
                "service imports are forbidden during initialization",
            ));
        }
        let slot = std::str::from_utf8(unsafe { slot.read() }?)
            .map_err(|_| Failure::new(Code::SchemaRejected, "invalid slot selector"))?;
        let key = std::str::from_utf8(unsafe { key.read() }?)
            .map_err(|_| Failure::new(Code::SchemaRejected, "invalid operation key"))?;
        let bytes = unsafe { input.read() }?;
        if key == "phenix.host.slots@1/count" {
            if Value::decode_profile(bytes, WireProfile::V2)? != Value::Null {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "slot count expects Null",
                ));
            }
            let count = kernel
                .prepared
                .slot_counts
                .get(&(record.caller.clone(), slot.into()))
                .ok_or_else(|| Failure::new(Code::Denied, "undeclared slot"))?;
            return Ok(Reply::state_value(0, Value::U64(*count)));
        }
        if key == "phenix.host.state@1/snapshot" {
            let Value::List(items) = Value::decode_profile(bytes, WireProfile::V2)? else {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "snapshot expects a ref list",
                ));
            };
            let locations = items
                .into_iter()
                .map(|v| match v {
                    Value::Ref(r) => Ok(*r),
                    _ => Err(Failure::new(Code::SchemaRejected, "snapshot expects refs")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if !slot.is_empty()
                || locations.is_empty()
                || locations.len() > phenix_contract::MAX_COUNT
                || !locations.windows(2).all(|p| p[0] < p[1])
            {
                return Err(Failure::new(Code::SchemaRejected, "snapshot count/order"));
            }
            let result = (|| {
                for r in &locations {
                    kernel.authorize_state(&record.caller, r, "get")?;
                }
                kernel.cells.borrow().snapshot(&locations)
            })();
            return Ok(match result {
                Ok(value) => Reply::state_value(0, value),
                Err(error) => Reply::state_value(1, error.to_value()),
            });
        }
        let identity = phenix_contract::import_identity(slot, key);
        let provider = kernel
            .prepared
            .bindings
            .get(&(record.caller.clone(), identity))
            .ok_or_else(|| Failure::new(Code::MissingBinding, "slot operation is not selected"))?;
        let (interface, operation) = key
            .split_once('/')
            .ok_or_else(|| Failure::new(Code::SchemaRejected, "invalid operation key"))?;
        let selection = Selection {
            interface: interface.into(),
            operation: operation.into(),
            provider: provider.clone(),
        };
        let outcome = kernel.dispatch_call(
            record.root,
            Caller::Plugin(&record.caller, slot),
            &selection,
            bytes,
            None,
            false,
        )?;
        Ok(match outcome {
            Call::Ready(Outcome::Success(v)) => Reply::state_value(0, v),
            Call::Ready(Outcome::Domain(v)) => Reply::state_value(1, v),
            Call::Pending(_) => unreachable!(),
        })
    })
}
