//! Authenticated front doors to the canonical runtime. Refusal must not supervise.
use super::*;
use crate::network::{
    BoundDeclaration, BoundOutput, CaptureHandle, IngressHandle, NetworkError, SourceHandle,
};

/// Checked configuration retains its generation proof until the actual start.
/// Dynamic SessionOptions maps remain available for validated symbolic authoring.
/// Bound entries override dynamic entries; the last bound entry for a path wins.
/// All selectors are validated before replacement or executable callbacks.
#[derive(Clone, Default)]
pub struct BoundSessionOptions {
    readiness: Vec<BoundConfiguration<Readiness>>,
    resources: Vec<BoundConfiguration<crate::ValidatedResourceSet>>,
    discard: Vec<(BoundDeclaration, String)>,
}

// The value and its caught panic payload must be destroyed before any of the
// declaration's known leases, including when options never reach start().
#[derive(Clone)]
struct BoundConfiguration<T> {
    value: Option<T>,
    dispose: fn(T) -> bool,
    declaration: BoundDeclaration,
}
impl<T> BoundConfiguration<T> {
    fn cleanup(&mut self) -> bool {
        self.value.take().is_some_and(self.dispose)
    }
}
impl<T> Drop for BoundConfiguration<T> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

struct BoundReadiness(BoundConfiguration<Readiness>);
impl BoundReadiness {
    fn policy(&self) -> &dyn ReadinessPolicy {
        self.0.value.as_ref().expect("configured readiness")
    }
}
impl ReadinessPolicy for BoundReadiness {
    fn declaration(&self) -> PolicyDeclaration {
        self.policy().declaration()
    }
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.policy().select(snapshot)
    }
    fn closure_ports(&self, lane: usize) -> Vec<String> {
        self.policy().closure_ports(lane)
    }
    fn notification_driven(&self) -> bool {
        self.policy().notification_driven()
    }
}

fn drop_catching<T>(value: T) -> bool {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(value))) {
        crate::discard_panic_payload(payload);
        true
    } else {
        false
    }
}

fn configuration_cleanup(failure: &mut Option<Failure>, path: &str, field: &str, panicked: bool) {
    if panicked {
        let secondary = RuntimeError::DestructorPanicked {
            stage: CleanupStage::Configuration,
        }
        .failure()
        .with_context(crate::FailureContext {
            path: Some(path.into()),
            field: Some(field.into()),
            ..Default::default()
        });
        *failure = Some(match failure.take() {
            Some(primary) => primary.with_cleanup(secondary),
            None => secondary,
        });
    }
}

fn lower_configuration<T>(
    entries: &mut Vec<BoundConfiguration<T>>,
    target: &mut BTreeMap<String, T>,
    field: &str,
    wrap: impl Fn(BoundConfiguration<T>) -> T,
    failure: &mut Option<Failure>,
) {
    let mut selected: BTreeMap<String, BoundConfiguration<T>> = BTreeMap::new();
    // Last bound registration wins, and bound values override dynamic maps.
    // Dispose shadowed bound values while their own declarations still pin code.
    for entry in std::mem::take(entries) {
        let path = entry.declaration.path().to_owned();
        if let Some(mut replaced) = selected.insert(path.clone(), entry) {
            configuration_cleanup(failure, &path, field, replaced.cleanup());
        }
    }
    for (path, entry) in selected {
        if let Some(replaced) = target.remove(&path) {
            configuration_cleanup(failure, &path, field, (entry.dispose)(replaced));
        }
        target.insert(path, wrap(entry));
    }
}

impl BoundSessionOptions {
    pub(super) fn execution_lease(&self) -> Option<Arc<dyn Send + Sync>> {
        let leases: Vec<_> = self
            .readiness
            .iter()
            .map(|entry| &entry.declaration)
            .chain(self.resources.iter().map(|entry| &entry.declaration))
            .chain(self.discard.iter().map(|(d, _)| d))
            .filter_map(BoundDeclaration::execution_lease)
            .collect();
        (!leases.is_empty()).then(|| Arc::new(leases) as Arc<dyn Send + Sync>)
    }
    pub fn readiness(&mut self, process: &BoundDeclaration, policy: Readiness) {
        self.readiness.push(BoundConfiguration {
            value: Some(policy),
            dispose: drop_catching,
            declaration: process.clone(),
        });
    }
    pub fn resources(
        &mut self,
        process: &BoundDeclaration,
        resources: crate::ValidatedResourceSet,
    ) {
        self.resources.push(BoundConfiguration {
            value: Some(resources),
            dispose: crate::ValidatedResourceSet::dispose_catching,
            declaration: process.clone(),
        });
    }
    pub fn discard_output<T>(
        &mut self,
        network: &CompiledNetwork,
        output: &BoundOutput<T>,
    ) -> Result<(), NetworkError> {
        self.discard.push((
            output.physical_declaration(network)?,
            output.resolve(network)?.1.into(),
        ));
        Ok(())
    }
    pub(super) fn apply(
        &mut self,
        network: &CompiledNetwork,
        readiness: &mut BTreeMap<String, Readiness>,
        resources: &mut BTreeMap<String, crate::ValidatedResourceSet>,
        discard: &mut BTreeSet<(String, String)>,
    ) -> Result<(), Failure> {
        // Validate every selector before lowering any of them or invoking factories.
        for declaration in self
            .readiness
            .iter()
            .map(|entry| &entry.declaration)
            .chain(self.resources.iter().map(|entry| &entry.declaration))
            .chain(self.discard.iter().map(|(d, _)| d))
        {
            declaration
                .process_path(network)
                .map_err(|error| Failure::external("invalid bound configuration", error))?;
        }
        let mut failure = None;
        lower_configuration(
            &mut self.readiness,
            readiness,
            "readiness",
            |entry| Readiness::from_policy(BoundReadiness(entry)),
            &mut failure,
        );
        lower_configuration(
            &mut self.resources,
            resources,
            "resources",
            |mut entry| entry.value.take().expect("configured resources"),
            &mut failure,
        );
        for (d, port) in &self.discard {
            discard.insert((d.path().into(), port.clone()));
        }
        failure.map_or(Ok(()), Err)
    }
}

impl NetworkSession {
    pub fn inject_bound<T: Send + Sync + 'static>(
        &mut self,
        ingress: &IngressHandle<T>,
        payload: T,
    ) -> Result<(), InjectionError> {
        self.inject_bound_value(ingress, ingress.value(payload))
    }
    pub fn inject_bound_value<T>(
        &mut self,
        ingress: &IngressHandle<T>,
        value: Value,
    ) -> Result<(), InjectionError> {
        let value = ingress.retain_value(value);
        if ingress.validate(&self.compiled).is_err() {
            return Err(InjectionError {
                reason: ConnectionError::ForeignTopology,
                value,
            });
        }
        self.inject(ingress.name(), value)
    }
    pub fn close_bound_input<T>(
        &mut self,
        ingress: &IngressHandle<T>,
    ) -> Result<(), ConnectionError> {
        ingress
            .validate(&self.compiled)
            .map_err(|_| ConnectionError::ForeignTopology)?;
        self.close_input(ingress.name())
    }
    /// An exact capture, `Some(capture)`, or `None` uses canonical request counts.
    pub fn begin_bound_request<'a>(
        &mut self,
        exact_output: impl Into<Option<&'a CaptureHandle>>,
    ) -> Result<RequestId, RequestError> {
        let exact_output = exact_output.into();
        if let Some(capture) = exact_output {
            capture
                .validate(&self.compiled)
                .map_err(|_| RequestError::Foreign)?;
        }
        self.begin_request(exact_output.map(CaptureHandle::name))
    }
    pub fn inject_bound_request<T: Send + Sync + 'static>(
        &mut self,
        id: RequestId,
        ingress: &IngressHandle<T>,
        payload: T,
    ) -> Result<(), RequestInjectionError> {
        self.inject_bound_request_value(id, ingress, ingress.value(payload))
    }
    pub fn inject_bound_request_value<T>(
        &mut self,
        id: RequestId,
        ingress: &IngressHandle<T>,
        value: Value,
    ) -> Result<(), RequestInjectionError> {
        let value = ingress.retain_value(value);
        let validation = ingress
            .validate(&self.compiled)
            .map_err(|_| RequestError::Foreign)
            .and_then(|()| self.check_request_admission(id).map(|_| ()));
        if let Err(reason) = validation {
            return Err(RequestInjectionError { reason, value });
        }
        self.inject_request(id, ingress.name(), value)
    }
    pub fn take_bound_output(
        &mut self,
        capture: &CaptureHandle,
    ) -> Result<Option<Packet>, ConnectionError> {
        capture
            .validate(&self.compiled)
            .map_err(|_| ConnectionError::ForeignTopology)?;
        self.take_output(capture.name())
            .map(|packet| packet.map(|packet| capture.retain_packet(packet)))
    }
    /// Relieve only unscoped observation backpressure. A packet owned by any
    /// live request remains available for its authenticated scoped capture read,
    /// even when the caller is trying to make progress on another request.
    ///
    /// This examines only the canonical FIFO head. Never skip or discard a
    /// request-owned packet to free capacity for an unrelated demand.
    pub fn take_bound_unscoped_output(
        &mut self,
        capture: &CaptureHandle,
    ) -> Result<Option<Packet>, ConnectionError> {
        capture
            .validate(&self.compiled)
            .map_err(|_| ConnectionError::ForeignTopology)?;
        self.supervise_configured_deferred();
        let index = *self
            .captures
            .get(capture.name())
            .ok_or(ConnectionError::InvalidDestination)?;
        if self.transport.queues()[index]
            .head()
            .is_some_and(|packet| packet.metadata().and_then(|meta| meta.request()).is_some())
        {
            return Ok(None);
        }
        self.take_capture(capture.name(), index)
            .map(|packet| packet.map(|packet| capture.retain_packet(packet)))
    }

    pub fn take_bound_request_output(
        &mut self,
        id: RequestId,
        capture: &CaptureHandle,
    ) -> Result<Option<Packet>, RequestError> {
        capture
            .validate(&self.compiled)
            .map_err(|_| RequestError::Foreign)?;
        self.check_request_capture(id)?;
        self.take_request_output(id, capture.name())
            .map(|packet| packet.map(|packet| capture.retain_packet(packet)))
    }
    pub fn validate_bound_capture(
        &self,
        id: RequestId,
        capture: &CaptureHandle,
        packet: &Packet,
    ) -> Result<(), RequestError> {
        capture
            .validate(&self.compiled)
            .map_err(|_| RequestError::Foreign)?;
        self.validate_capture(id, capture.name(), packet)
    }
    pub fn bound_output_closed(&self, capture: &CaptureHandle) -> Result<bool, ConnectionError> {
        capture
            .validate(&self.compiled)
            .map_err(|_| ConnectionError::ForeignTopology)?;
        self.output_closed(capture.name())
    }
    pub fn start_bound_request_source(
        &mut self,
        id: RequestId,
        source: &SourceHandle,
    ) -> Result<(), RequestError> {
        source
            .validate(&self.compiled)
            .map_err(|_| RequestError::Foreign)?;
        self.check_request_admission(id)?;
        self.start_request_source_with_leases(id, source.path(), source.known_leases())
    }
    pub fn bound_wake_handle(
        &self,
        declaration: &BoundDeclaration,
    ) -> Result<Option<WakeHandle>, NetworkError> {
        Ok(self.wake_handle(declaration.process_path(&self.compiled)?))
    }
    pub fn bound_process_notifier(
        &self,
        declaration: &BoundDeclaration,
    ) -> Result<Option<ProcessNotifier>, NetworkError> {
        Ok(self.process_notifier(declaration.process_path(&self.compiled)?))
    }
}
