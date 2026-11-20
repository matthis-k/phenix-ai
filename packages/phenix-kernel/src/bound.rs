//! Bound front doors authenticate before exclusive access or notification.
use crate::{ActiveSessionCall, Kernel, SessionAccessError};
use phenix_graph::{
    BoundDeclaration, CaptureHandle, IngressHandle, NetworkError, SourceHandle, Value,
    connection::{ConnectionError, Packet},
    runtime::{InjectionError, RequestError, RequestId, RequestInjectionError},
};

impl Kernel {
    /// Conversion retains the handle's known owners and runs before session access.
    pub fn inject_bound<T: Send + Sync + 'static>(
        &self,
        ingress: &IngressHandle<T>,
        payload: T,
    ) -> Result<(), InjectionError> {
        self.inject_bound_value(ingress, ingress.value(payload))
    }

    /// Already constructed values can be retried without invoking the codec again.
    pub fn inject_bound_value<T: 'static>(
        &self,
        ingress: &IngressHandle<T>,
        value: Value,
    ) -> Result<(), InjectionError> {
        // Retain owners even on foreign, unstarted, busy and reentrant refusals.
        let value = ingress.retain_value(value);
        if ingress.validate(&self.compiled).is_err() {
            return Err(InjectionError {
                reason: ConnectionError::ForeignTopology,
                value,
            });
        }
        self.inject(ingress.name(), value)
    }

    pub fn inject_bound_request<T: Send + Sync + 'static>(
        &self,
        id: RequestId,
        ingress: &IngressHandle<T>,
        payload: T,
    ) -> Result<(), RequestInjectionError> {
        self.inject_bound_request_value(id, ingress, ingress.value(payload))
    }

    pub fn inject_bound_request_value<T: 'static>(
        &self,
        id: RequestId,
        ingress: &IngressHandle<T>,
        value: Value,
    ) -> Result<(), RequestInjectionError> {
        let value = ingress.retain_value(value);
        if ingress.validate(&self.compiled).is_err() {
            return Err(RequestInjectionError {
                reason: RequestError::Foreign,
                value,
            });
        }
        if !self.started {
            return Err(RequestInjectionError {
                reason: RequestError::Transport(ConnectionError::Cancelled),
                value,
            });
        }
        let _active = match ActiveSessionCall::enter(self.session_identity) {
            Ok(active) => active,
            Err(()) => {
                return Err(RequestInjectionError {
                    reason: RequestError::Transport(ConnectionError::Reentrant),
                    value,
                });
            }
        };
        let Some(mut session) = self.lease_session() else {
            return Err(RequestInjectionError {
                reason: RequestError::Transport(ConnectionError::WouldBlock),
                value,
            });
        };
        let result = session.inject_bound_request_value(id, ingress, value);
        if result.is_ok() {
            session.notify_on_drop();
        }
        result
    }

    pub fn begin_bound_request<'a>(
        &self,
        exact_output: impl Into<Option<&'a CaptureHandle>>,
    ) -> Result<Result<RequestId, RequestError>, SessionAccessError> {
        let exact_output = exact_output.into();
        if exact_output.is_some_and(|capture| capture.validate(&self.compiled).is_err()) {
            return Ok(Err(RequestError::Foreign));
        }
        self.with_session(|session| session.begin_bound_request(exact_output))
    }

    pub fn close_bound_input<T>(
        &self,
        ingress: &IngressHandle<T>,
    ) -> Result<Result<(), ConnectionError>, SessionAccessError> {
        if ingress.validate(&self.compiled).is_err() {
            return Ok(Err(ConnectionError::ForeignTopology));
        }
        self.with_session(|session| session.close_bound_input(ingress))
    }

    pub fn take_bound_output(
        &self,
        capture: &CaptureHandle,
    ) -> Result<Result<Option<Packet>, ConnectionError>, SessionAccessError> {
        if capture.validate(&self.compiled).is_err() {
            return Ok(Err(ConnectionError::ForeignTopology));
        }
        self.with_session(|session| session.take_bound_output(capture))
    }

    pub fn take_bound_request_output(
        &self,
        id: RequestId,
        capture: &CaptureHandle,
    ) -> Result<Result<Option<Packet>, RequestError>, SessionAccessError> {
        if capture.validate(&self.compiled).is_err() {
            return Ok(Err(RequestError::Foreign));
        }
        self.with_session(|session| session.take_bound_request_output(id, capture))
    }

    pub fn validate_bound_capture(
        &self,
        id: RequestId,
        capture: &CaptureHandle,
        packet: &Packet,
    ) -> Result<Result<(), RequestError>, SessionAccessError> {
        if capture.validate(&self.compiled).is_err() {
            return Ok(Err(RequestError::Foreign));
        }
        self.with_session(|session| session.validate_bound_capture(id, capture, packet))
    }

    pub fn bound_output_closed(
        &self,
        capture: &CaptureHandle,
    ) -> Result<Result<bool, ConnectionError>, SessionAccessError> {
        if capture.validate(&self.compiled).is_err() {
            return Ok(Err(ConnectionError::ForeignTopology));
        }
        self.with_session(|session| session.bound_output_closed(capture))
    }

    pub fn start_bound_request_source(
        &self,
        id: RequestId,
        source: &SourceHandle,
    ) -> Result<Result<(), RequestError>, SessionAccessError> {
        if source.validate(&self.compiled).is_err() {
            return Ok(Err(RequestError::Foreign));
        }
        self.with_session(|session| session.start_bound_request_source(id, source))
    }

    pub fn bound_wake_handle(
        &self,
        declaration: &BoundDeclaration,
    ) -> Result<Result<Option<phenix_graph::runtime::WakeHandle>, NetworkError>, SessionAccessError>
    {
        if let Err(error) = declaration.validate(&self.compiled) {
            return Ok(Err(error));
        }
        self.with_session(|session| session.bound_wake_handle(declaration))
    }

    pub fn bound_process_notifier(
        &self,
        declaration: &BoundDeclaration,
    ) -> Result<
        Result<Option<phenix_graph::runtime::ProcessNotifier>, NetworkError>,
        SessionAccessError,
    > {
        if let Err(error) = declaration.validate(&self.compiled) {
            return Ok(Err(error));
        }
        self.with_session(|session| session.bound_process_notifier(declaration))
    }
}
