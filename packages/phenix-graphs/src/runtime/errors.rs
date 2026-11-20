use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupStage {
    Configuration,
    Request,
    Failure,
    Cancellation,
    LateReturn,
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputFailureReason {
    Abandoned,
    Stranded,
}

/// Native runtime faults contain host data, never formatted application causes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    UnavailableProcess,
    UnknownConfiguration {
        path: String,
    },
    InvalidDiscard {
        path: String,
        output: String,
    },
    LaneCapacityOverflow,
    CorrelationBound {
        path: String,
        lanes: usize,
        requests: usize,
    },
    UnknownOutput {
        process: usize,
        path: String,
        output: String,
    },
    EmissionType {
        process: usize,
        path: String,
        output: String,
        expected: crate::TypeIdentity,
        actual: crate::TypeIdentity,
    },
    SelectionBudget {
        budget: usize,
        actual: usize,
    },
    UnknownSelection {
        position: usize,
    },
    EmptySelection {
        position: usize,
    },
    UnconnectedSelection {
        path: String,
        input: String,
    },
    DuplicateSelection {
        path: String,
        input: String,
    },
    UnconnectedOutput {
        path: String,
        output: String,
    },
    UnknownWaitInput {
        port: String,
    },
    DeferredTimeout {
        process: usize,
        path: String,
        request: Option<RequestId>,
        ticket: StepTicket,
        timeout: Duration,
    },
    ActiveRequestCancelled,
    Input {
        process: usize,
        path: String,
        reason: InputFailureReason,
        packets: usize,
        ports: Vec<(String, usize)>,
    },
    DestructorPanicked {
        stage: CleanupStage,
    },
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnavailableProcess => f.write_str("process factory unavailable"),
            Self::UnknownConfiguration { path } => {
                write!(f, "unknown process configuration: {path}")
            }
            Self::InvalidDiscard { path, output } => {
                write!(f, "invalid discard output: {path}.{output}")
            }
            Self::LaneCapacityOverflow => {
                f.write_str("bounded request lane packet capacity overflows usize")
            }
            Self::CorrelationBound {
                path,
                lanes,
                requests,
            } => write!(
                f,
                "insufficient correlation bound: {path} has {lanes} lanes for {requests} inflight requests; head blocking is not supported beyond this bound"
            ),
            Self::UnknownOutput {
                process,
                path,
                output,
            } => write!(
                f,
                "invalid emission from process {process} ({path}) output {output:?}: unknown output"
            ),
            Self::EmissionType {
                process,
                path,
                output,
                expected,
                actual,
            } => write!(
                f,
                "invalid emission from process {process} ({path}) output {output:?}: incompatible value identity (expected {expected:?}, actual {actual:?})"
            ),
            Self::SelectionBudget { budget, .. } => write!(
                f,
                "selection exceeds compiled outgoing connection budget ({budget})"
            ),
            Self::UnknownSelection { position } => {
                write!(f, "unknown selected input at destination {position}")
            }
            Self::EmptySelection { .. } => {
                f.write_str("selected input has no physical destinations")
            }
            Self::UnconnectedSelection { path, input } => write!(
                f,
                "selected input {path}.{input} is not connected to this output"
            ),
            Self::DuplicateSelection { path, input } => write!(
                f,
                "duplicate selected input after alias resolution: {path}.{input}"
            ),
            Self::UnconnectedOutput { path, output } => write!(
                f,
                "unconnected output {path}.{output} requires explicit discard_outputs"
            ),
            Self::UnknownWaitInput { port } => write!(f, "unknown WaitInput port: {port}"),
            Self::DeferredTimeout {
                process,
                path,
                request,
                ticket,
                ..
            } => write!(
                f,
                "deferred job timeout for process {process} ({path}, request={request:?}) at activation {} step {}",
                ticket.activation(),
                ticket.sequence()
            ),
            Self::ActiveRequestCancelled => {
                f.write_str("active request cancelled without RestartAffectedProcess policy")
            }
            Self::Input {
                path,
                reason,
                packets,
                ports,
                ..
            } => {
                let reason = match reason {
                    InputFailureReason::Abandoned => "abandoned accepted input on Finished",
                    InputFailureReason::Stranded => {
                        "stranded input: all writers closed and no legal reservation"
                    }
                };
                write!(
                    f,
                    "{reason} at {path}: {packets} accepted undelivered packets ("
                )?;
                for (index, (port, count)) in ports.iter().enumerate() {
                    if index != 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{port}={count}")?;
                }
                f.write_str(")")
            }
            Self::DestructorPanicked {
                stage: CleanupStage::Finish,
            } => f.write_str("owned destructor panicked while finishing"),
            Self::DestructorPanicked { stage } => {
                write!(f, "destructor panicked during {stage:?} cleanup")
            }
        }
    }
}
impl std::error::Error for RuntimeError {}
impl RuntimeError {
    pub(super) fn failure(self) -> Failure {
        let kind = if matches!(self, Self::DestructorPanicked { .. }) {
            FailureKind::Cleanup
        } else {
            FailureKind::Framework
        };
        Failure::external(format!("{self}"), self).with_kind(kind)
    }
}
