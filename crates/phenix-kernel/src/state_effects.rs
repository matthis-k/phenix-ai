//! Authenticated state effects and ABI 3 imports enter the existing runtime.
use crate::{
    resolve::{CellOperation, Selection},
    runtime::{authenticate_import_scope, authenticate_scope, Caller, Kernel},
    Call, Outcome,
};
use phenix_contract::{
    abi::{Bytes, Reply},
    Code, CompareExchange, Failure, StateRequest, Value, ValueAccessError as Error, ValueLocation,
};

impl Kernel {
    pub(super) fn require_property_resource(
        &self,
        selection: &Selection,
        authority: &phenix_contract::Authority,
    ) -> Result<(), Failure> {
        let resource = selection.interface.strip_suffix("@1").ok_or_else(|| {
            Failure::new(
                Code::DescriptorInvalid,
                "invalid prepared property identity",
            )
        })?;
        if !authority.resources.contains(resource) {
            return self.fail(Code::Denied, "state property resource is not granted");
        }
        Ok(())
    }
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
            phenix_contract::property_contract(owner, &property.id).map_err(|_| Error::Denied)?;
        let key = format!("{interface}/{operation}");
        let selected = caller == "root"
            || caller == owner
            || self
                .prepared
                .bindings
                .iter()
                .any(|((principal, identity), provider)| {
                    principal == caller
                        && provider == owner
                        && identity
                            .split_once(':')
                            .is_some_and(|(_, operation)| operation == key)
                });
        if !enabled || !selected {
            let _ = self.fail::<()>(Code::Denied, "state access is not selected");
            return Err(Error::Denied);
        }
        let scope = self
            .effect_scope(caller, owner, &key, None)
            .map_err(|_| Error::Denied)?;
        self.require_property_resource(
            &Selection {
                interface,
                operation: operation.into(),
                provider: owner.into(),
            },
            &scope.authority,
        )
        .map_err(|_| Error::Denied)
    }
    fn owner_readonly(&self, caller: &str, location: &ValueLocation) -> Result<bool, Error> {
        let cells = self.cells.borrow();
        let (owner, property) = cells.identity(location)?;
        Ok(owner == caller && !property.set)
    }

    /// Owner publication is separate from the public property `set` contract.
    ///
    /// Only the currently executing owner may publish a new observation.
    /// The executing frame must still carry the property resource grant.
    /// In particular, publication does not make a read-only property writable
    /// to imported consumers or to the root.
    fn authorize_owner_publish(&self, caller: &str, location: &ValueLocation) -> Result<(), Error> {
        let cells = self.cells.borrow();
        let (owner, property) = cells.identity(location)?;
        if owner != caller || (!property.get && !property.watch) {
            return Err(Error::Denied);
        }
        let interface =
            phenix_contract::property_contract(owner, &property.id).map_err(|_| Error::Denied)?;
        let owner = owner.to_owned();
        drop(cells);

        let authority = {
            let state = self.state.borrow();
            let Some(frame) = state.stack.last() else {
                return Err(Error::Denied);
            };
            if frame.provider != owner || state.stack.iter().any(|frame| frame.revoked) {
                return Err(Error::Denied);
            }
            frame.authority.clone()
        };
        self.require_property_resource(
            &Selection {
                interface,
                operation: "get".into(),
                provider: owner,
            },
            &authority,
        )
        .map_err(|_| Error::Denied)
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
        let result = (|| {
            match &request {
                StateRequest::Create { .. } => return Err(Error::Denied),
                StateRequest::Read(r) => self.authorize_state(caller, r, "get")?,
                StateRequest::CompareExchange(edit) => {
                    // The owner may publish a read-only observation under its
                    // active resource authority. All other writes continue to
                    // require a selected public `set` operation.
                    if self.owner_readonly(caller, &edit.location)? {
                        self.authorize_owner_publish(caller, &edit.location)?;
                    } else {
                        self.authorize_state(caller, &edit.location, "set")?;
                    }
                }
                StateRequest::Batch(edits) => {
                    for edit in edits {
                        if self.owner_readonly(caller, &edit.location)? {
                            self.authorize_owner_publish(caller, &edit.location)?;
                        } else {
                            self.authorize_state(caller, &edit.location, "set")?;
                        }
                    }
                }
                StateRequest::Subscribe(r) => self.authorize_state(caller, r, "watch")?,
                StateRequest::Next(id) | StateRequest::Unsubscribe(id) => {
                    let r = self.cells.borrow().cursor(root, caller, ticket, *id)?;
                    self.authorize_state(caller, &r, "watch")?;
                }
            }
            self.apply_state_request(root, caller, request)
        })();
        Ok(result)
    }
    // Both authenticated host requests and prepared cell targets reach this
    // mutation point only after their complete operation/resource checks.
    fn apply_state_request(
        &self,
        root: u64,
        caller: &str,
        request: StateRequest,
    ) -> Result<Value, Error> {
        let ticket = self.state.borrow().stack.last().and_then(|f| f.ticket);
        match request {
            StateRequest::Create { .. } => Err(Error::Denied),
            StateRequest::Read(r) => self.cells.borrow().read(&r),
            StateRequest::CompareExchange(edit) => self.cells.borrow_mut().compare_exchange(edit),
            StateRequest::Batch(edits) => self.cells.borrow_mut().commit(&edits),
            StateRequest::Subscribe(r) => {
                self.cells.borrow_mut().subscribe(root, caller, ticket, r)
            }
            StateRequest::Next(id) | StateRequest::Unsubscribe(id) => {
                if matches!(request, StateRequest::Next(_)) {
                    self.cells.borrow_mut().next(root, caller, ticket, id)
                } else {
                    self.cells
                        .borrow_mut()
                        .unsubscribe(root, caller, ticket, id)
                }
            }
        }
    }
    pub(super) fn cell_dispatch(
        &self,
        root: u64,
        caller: &str,
        selection: &Selection,
        property: &str,
        operation: CellOperation,
        request: &Value,
    ) -> Result<Outcome, Failure> {
        let state_request = match (operation, request) {
            (CellOperation::Get, Value::Ref(r)) => StateRequest::Read(**r),
            (CellOperation::Watch, Value::Ref(r)) => StateRequest::Subscribe(**r),
            (CellOperation::Set, Value::Map(m)) => {
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
        if !self
            .cells
            .borrow()
            .identity(r)
            .is_ok_and(|(owner, p)| owner == selection.provider && p.id == property)
        {
            return Ok(Outcome::Domain(Value::String("StaleReference".into())));
        }
        Ok(
            match self.apply_state_request(root, caller, state_request) {
                Ok(Value::List(v)) => Outcome::Success(match operation {
                    CellOperation::Get => v[2].clone(),
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
        )
    }
}
fn protected(body: impl FnOnce() -> Result<Reply, Failure>) -> Reply {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(Ok(reply)) => reply,
        Ok(Err(failure)) => Reply::value(2, failure.to_value()),
        Err(_) => Reply::value(
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
                Ok(value) => Reply::value(0, value),
                Err(error) => Reply::value(1, error.to_value()),
            },
        )
    })
}
pub(super) unsafe extern "C" fn invoke(scope: u64, slot: Bytes, key: Bytes, input: Bytes) -> Reply {
    protected(|| {
        let record = authenticate_import_scope(scope)?;
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
        let cleanup =
            key == "phenix.host.pending.poll.v2" || key == "phenix.host.pending.cancel.v2";
        if kernel
            .state
            .borrow()
            .stack
            .last()
            .is_some_and(|frame| frame.revoked)
            && !cleanup
        {
            return Err(Failure::new(Code::ScopeRejected, "cancelled host scope"));
        }
        let bytes = unsafe { input.read() }?;
        if key.starts_with("phenix.host.pending.") {
            let parent = kernel
                .state
                .borrow()
                .stack
                .last()
                .and_then(|frame| frame.ticket)
                .ok_or_else(|| {
                    Failure::new(Code::ScopeRejected, "pending command needs a native ticket")
                })?;
            return Ok(
                match kernel.import_pending(record.root, &record.caller, slot, parent, key, bytes) {
                    Ok(reply) => reply,
                    Err(error) => Reply::value(5, error.to_value()),
                },
            );
        }
        if key == phenix_contract::HOST_GRAPH_EMIT {
            if !slot.is_empty() {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "graph emission expects no import slot",
                ));
            }
            let Value::Map(mut request) = Value::decode(bytes)? else {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "graph emission expects an event and payload map",
                ));
            };
            let Some(Value::String(id)) = request.remove("event") else {
                return Err(Failure::new(Code::SchemaRejected, "graph event ID missing"));
            };
            let payload = request.remove("payload").ok_or_else(|| {
                Failure::new(Code::SchemaRejected, "graph event payload missing")
            })?;
            if !request.is_empty() {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "unexpected graph emission fields",
                ));
            }
            kernel.queue_graph_event(record.root, &record.caller, &id, payload)?;
            return Ok(Reply::value(0, Value::Null));
        }
        if key == phenix_contract::RESOURCE_CONTEXT {
            if !slot.is_empty() || Value::decode(bytes)? != Value::Null {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "resource context expects Null and no slot",
                ));
            }
            let resource = kernel
                .state
                .borrow()
                .stack
                .last()
                .and_then(|frame| frame.resource.clone());
            return Ok(Reply::value(0, resource.map_or(Value::Null, Value::String)));
        }
        if key == "phenix.host.slots@1/count" {
            if Value::decode(bytes)? != Value::Null {
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
            return Ok(Reply::value(0, Value::U64(*count)));
        }
        if key == "phenix.host.slots@1/selected" {
            let Value::String(operation) = Value::decode(bytes)? else {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "selected provider query expects an operation key",
                ));
            };
            if !phenix_contract::operation_key(&operation) {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "invalid selected operation key",
                ));
            }
            let base = slot.split_once('#').map_or(slot, |(base, _)| base);
            if !kernel
                .prepared
                .slot_counts
                .contains_key(&(record.caller.clone(), base.into()))
            {
                return Err(Failure::new(Code::Denied, "undeclared slot"));
            }
            let identity = phenix_contract::import_identity(slot, &operation);
            let provider = kernel
                .prepared
                .bindings
                .get(&(record.caller.clone(), identity))
                .ok_or_else(|| Failure::new(Code::MissingBinding, "slot operation not selected"))?;
            return Ok(Reply::value(0, Value::String(provider.clone())));
        }
        if key == "phenix.host.state@1/snapshot" {
            let Value::List(items) = Value::decode(bytes)? else {
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
                Ok(value) => Reply::value(0, value),
                Err(error) => Reply::value(1, error.to_value()),
            });
        }
        let locate = key == "phenix.host.property.v2";
        let property_key = if locate {
            let Value::String(key) = Value::decode(bytes)? else {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "property lookup expects an operation key",
                ));
            };
            Some(key)
        } else {
            None
        };
        let key = property_key.as_deref().unwrap_or(key);
        let scoped = if !locate && key == phenix_contract::SCOPED_IMPORT {
            Some(phenix_contract::ScopedRequest::decode(bytes)?)
        } else {
            None
        };
        let (key, bytes, restriction) = match &scoped {
            Some((key, input, scope)) => (key.as_str(), input.as_slice(), Some(scope)),
            None => (key, bytes, None),
        };
        let identity = phenix_contract::import_identity(slot, key);
        let provider = match kernel
            .prepared
            .bindings
            .get(&(record.caller.clone(), identity))
        {
            Some(provider) => provider,
            None => {
                let base = slot.split_once('#').map_or(slot, |(base, _)| base);
                let declared = kernel
                    .prepared
                    .slot_counts
                    .contains_key(&(record.caller.clone(), base.into()));
                let failure = kernel
                    .fail::<()>(
                        if declared {
                            Code::MissingBinding
                        } else {
                            Code::Denied
                        },
                        "slot operation is not selected",
                    )
                    .unwrap_err();
                kernel.observe_import_failure(&record.caller, Some(key), None, &failure);
                return Err(failure);
            }
        };
        let (interface, operation) = key
            .split_once('/')
            .ok_or_else(|| Failure::new(Code::SchemaRejected, "invalid operation key"))?;
        let selection = Selection {
            interface: interface.into(),
            operation: operation.into(),
            provider: provider.clone(),
        };
        if locate {
            let operation = kernel.prepared.operation(&selection)?;
            let crate::resolve::EffectTarget::Cell { property, .. } = &operation.target else {
                return Err(Failure::new(
                    Code::Denied,
                    "selected operation is not a property",
                ));
            };
            let location = kernel
                .cells
                .borrow()
                .location(provider, property)
                .ok_or_else(|| {
                    Failure::new(Code::ScopeRejected, "selected property is not live")
                })?;
            kernel
                .authorize_state(&record.caller, &location, &selection.operation)
                .map_err(|_| Failure::new(Code::Denied, "property discovery is not authorized"))?;
            return Ok(Reply::value(0, Value::Ref(Box::new(location))));
        }
        let outcome = kernel.dispatch_call(
            record.root,
            Caller::Plugin(&record.caller, slot),
            &selection,
            bytes,
            restriction,
            false,
        );
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(failure) => {
                kernel.observe_import_failure(&record.caller, Some(key), Some(provider), &failure);
                return Err(failure);
            }
        };
        Ok(match outcome {
            Call::Ready(Outcome::Success(v)) => Reply::value(0, v),
            Call::Ready(Outcome::Domain(v)) => Reply::value(1, v),
            Call::Pending(_) => unreachable!(),
        })
    })
}
