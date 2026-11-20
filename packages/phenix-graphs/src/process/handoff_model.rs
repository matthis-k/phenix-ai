//! Bounded sequential histories over the real owned handoff.
//!
//! The reference rules below are an independent reading of the public state
//! contract. They are deliberately not derived from `ProcessHandoff` methods.
use super::*;
use crate::runtime::Readiness;
use std::error::Error;
use std::num::NonZeroUsize;
use std::sync::Weak;
use std::sync::atomic::{AtomicUsize, Ordering};

const ANCHORS: usize = 21;
const SUFFIXES_PER_ANCHOR: usize = 73;
const HISTORIES: usize = ANCHORS * SUFFIXES_PER_ANCHOR;
const OPERATION_ATTEMPTS: usize = ANCHORS * (8 + 2 * 64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    InitialIdle,
    PostCompleteIdle,
    FreshReady,
    RejectedReady,
    YieldReady,
    PublicationCommittedReady,
    ResumeInputReady,
    ResumeWakeReady,
    DispatchedUnrun,
    DispatchedCompletion,
    PublicationPending,
    WaitInput,
    WaitInputs,
    WaitWake,
    FinishedStep,
    FinishedIdle,
    FailedStep,
    FailedPublication,
    CancelledRunnerPresent,
    CancelledRunnerAbsent,
    ReplacementFresh,
}

impl Anchor {
    const ALL: [Self; ANCHORS] = [
        Self::InitialIdle,
        Self::PostCompleteIdle,
        Self::FreshReady,
        Self::RejectedReady,
        Self::YieldReady,
        Self::PublicationCommittedReady,
        Self::ResumeInputReady,
        Self::ResumeWakeReady,
        Self::DispatchedUnrun,
        Self::DispatchedCompletion,
        Self::PublicationPending,
        Self::WaitInput,
        Self::WaitInputs,
        Self::WaitWake,
        Self::FinishedStep,
        Self::FinishedIdle,
        Self::FailedStep,
        Self::FailedPublication,
        Self::CancelledRunnerPresent,
        Self::CancelledRunnerAbsent,
        Self::ReplacementFresh,
    ];

    fn build(self) -> (Real, Oracle) {
        let mut real = Real::new(self.script());
        let mut model = Oracle::new(HandoffState::Idle);
        match self {
            Self::InitialIdle => {}
            Self::PostCompleteIdle => {
                real.admit();
                real.dispatch();
                real.run();
                assert!(matches!(real.settle(), Settled::Step(Step::Complete)));
                model.admit();
                model.dispatch();
                model.settle(HandoffState::Idle, false);
                model.effects = 1;
                model.complete_activation();
            }
            Self::FreshReady => {
                real.admit();
                model.admit();
            }
            Self::RejectedReady => {
                real.admit();
                real.dispatch();
                real.reject();
                model.admit();
                model.dispatch();
                model.state = HandoffState::Ready;
                model.outstanding = None;
                model.runner = RunnerKind::None;
            }
            Self::YieldReady => {
                real.admit();
                real.dispatch();
                real.run();
                assert!(matches!(real.settle(), Settled::Step(Step::Yield)));
                model.admit();
                model.dispatch();
                model.settle(HandoffState::Ready, false);
                model.effects = 1;
            }
            Self::PublicationCommittedReady => {
                real.admit();
                real.dispatch();
                real.run();
                assert!(matches!(real.settle(), Settled::Emission));
                real.handoff.publication_committed().unwrap();
                model.admit();
                model.dispatch();
                model.settle(HandoffState::PublicationPending, true);
                model.effects = 1;
                model.state = HandoffState::Ready;
            }
            Self::ResumeInputReady => {
                real.admit();
                real.dispatch();
                real.run();
                assert!(matches!(
                    real.settle(),
                    Settled::Step(Step::WaitInput(name)) if name == "next"
                ));
                real.handoff.resume_input(additional_context()).unwrap();
                model.admit();
                model.dispatch();
                model.settle(HandoffState::WaitingInput, false);
                model.effects = 1;
                model.state = HandoffState::Ready;
            }
            Self::ResumeWakeReady => {
                real.admit();
                real.dispatch();
                real.run();
                assert!(matches!(real.settle(), Settled::Step(Step::WaitWake)));
                real.handoff.resume_wake().unwrap();
                model.admit();
                model.dispatch();
                model.settle(HandoffState::WaitingWake, false);
                model.effects = 1;
                model.state = HandoffState::Ready;
            }
            Self::DispatchedUnrun => {
                real.admit();
                real.dispatch();
                model.admit();
                model.dispatch();
            }
            Self::DispatchedCompletion => {
                real.admit();
                real.dispatch();
                real.run();
                model.admit();
                model.dispatch();
                model.runner = RunnerKind::Completion;
                model.effects = 1;
            }
            Self::PublicationPending => {
                real.admit();
                real.dispatch();
                real.run();
                assert!(matches!(real.settle(), Settled::Emission));
                model.admit();
                model.dispatch();
                model.settle(HandoffState::PublicationPending, true);
                model.effects = 1;
            }
            Self::WaitInput | Self::WaitInputs | Self::WaitWake => {
                real.admit();
                real.dispatch();
                real.run();
                let state = if self == Self::WaitWake {
                    assert!(matches!(real.settle(), Settled::Step(Step::WaitWake)));
                    HandoffState::WaitingWake
                } else if self == Self::WaitInput {
                    assert!(matches!(
                        real.settle(),
                        Settled::Step(Step::WaitInput(name)) if name == "next"
                    ));
                    HandoffState::WaitingInput
                } else {
                    assert!(matches!(real.settle(), Settled::Step(Step::WaitInputs(_))));
                    HandoffState::WaitingInput
                };
                model.admit();
                model.dispatch();
                model.settle(state, false);
                model.effects = 1;
            }
            Self::FinishedStep => {
                real.admit();
                real.dispatch();
                real.run();
                assert!(matches!(real.settle(), Settled::Step(Step::Finished)));
                model.admit();
                model.dispatch();
                model.settle(HandoffState::Finished, false);
                model.effects = 1;
                model.clean_up();
            }
            Self::FinishedIdle => {
                real.handoff.finish_idle().unwrap();
                model.state = HandoffState::Finished;
                model.clean_up();
            }
            Self::FailedStep => {
                real.admit();
                real.dispatch();
                real.run();
                let Settled::Failure(failure) = real.settle() else {
                    panic!("FAILED_STEP did not return its scripted failure")
                };
                assert_eq!(failure.summary(), "scripted compact failure");
                assert_eq!(failure.context().phase, Some(CallbackPhase::Step));
                assert_eq!(
                    failure
                        .source()
                        .and_then(|source| source.downcast_ref::<CompactSource>())
                        .map(|source| source.marker),
                    Some(0xc04d)
                );
                model.admit();
                model.dispatch();
                model.settle(HandoffState::Failed, false);
                model.effects = 1;
                model.clean_up();
            }
            Self::FailedPublication => {
                real.admit();
                real.dispatch();
                real.run();
                assert!(matches!(real.settle(), Settled::Emission));
                real.handoff.publication_failed().unwrap();
                model.admit();
                model.dispatch();
                model.settle(HandoffState::PublicationPending, true);
                model.effects = 1;
                model.state = HandoffState::Failed;
                model.clean_up();
            }
            Self::CancelledRunnerPresent => {
                real.admit();
                real.dispatch();
                let expected = real.handoff.outstanding().unwrap();
                assert_eq!(real.handoff.cancel().unwrap(), Some(expected));
                model.admit();
                model.dispatch();
                model.state = HandoffState::Cancelled;
                model.outstanding = None;
                model.lease_alive = true;
            }
            Self::CancelledRunnerAbsent => {
                real.admit();
                model.admit();
                assert_eq!(real.handoff.cancel().unwrap(), None);
                model.state = HandoffState::Cancelled;
                model.outstanding = None;
                model.clean_up();
            }
            Self::ReplacementFresh => {
                real.admit();
                let old_effects = real.probes.effects.clone();
                let old_drops = real.probes.process_drops.clone();
                real.dispatch();
                let old_ticket = real.handoff.outstanding().unwrap();
                let old_job = match real.runner.take() {
                    Some(Runner::Job(job)) => job,
                    _ => unreachable!(),
                };
                assert_eq!(real.handoff.cancel().unwrap(), Some(old_ticket));
                let old_handoff = std::mem::replace(
                    &mut real.handoff,
                    ProcessHandoff::new(Box::new(UnreachableProcess)),
                );
                let (process, probes, lease) = scripted(self.script());
                let replacement = old_handoff.replacement(process);
                drop(old_handoff);
                real.handoff = replacement.with_lease(lease);
                real.probes = probes;
                real.retired = Some(Retired {
                    job: old_job,
                    effects: old_effects,
                    process_drops: old_drops,
                });
                model.activation = old_ticket.activation();
                model.sequence = old_ticket.sequence();
                model.state = HandoffState::Idle;
                model.provenance = "replacement fresh owner";
            }
        }
        real.assert_matches(self, &model);
        (real, model)
    }

    fn script(self) -> Script {
        match self {
            Self::YieldReady | Self::DispatchedCompletion => vec![Action::Yield, Action::Complete],
            Self::PublicationCommittedReady
            | Self::PublicationPending
            | Self::FailedPublication => vec![Action::Emit, Action::Complete],
            Self::ResumeInputReady | Self::WaitInput => {
                vec![Action::WaitInput, Action::Complete]
            }
            Self::WaitInputs => vec![Action::WaitInputs, Action::Complete],
            Self::ResumeWakeReady | Self::WaitWake => vec![Action::WaitWake, Action::Complete],
            Self::FinishedStep => vec![Action::Finished],
            Self::FailedStep => vec![Action::Fail],
            Self::CancelledRunnerPresent => vec![Action::Yield],
            Self::PostCompleteIdle => vec![Action::Complete, Action::Complete],
            Self::ReplacementFresh => vec![Action::Complete],
            _ => vec![Action::Complete],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Emit,
    Yield,
    WaitInput,
    WaitInputs,
    WaitWake,
    Complete,
    Finished,
    Fail,
}
impl Action {
    const ALL: [Self; 8] = [
        Self::Emit,
        Self::Yield,
        Self::WaitInput,
        Self::WaitInputs,
        Self::WaitWake,
        Self::Complete,
        Self::Finished,
        Self::Fail,
    ];

    fn continued_script(self) -> Script {
        match self {
            Self::Finished | Self::Fail => vec![self],
            _ => vec![self, Self::Complete],
        }
    }
}
type Script = Vec<Action>;

#[derive(Debug)]
struct CompactSource {
    marker: u64,
}
impl fmt::Display for CompactSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "compact source {:x}", self.marker)
    }
}
impl Error for CompactSource {}

struct Scripted {
    script: Script,
    position: usize,
    probes: Probes,
}
impl Process for Scripted {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        assert!(self.probes.lease.upgrade().is_some());
        let action = self.script[self.position];
        self.position += 1;
        self.probes.effects.fetch_add(1, Ordering::SeqCst);
        Ok(match action {
            Action::Emit => Step::Emit(ProcessEmission::new("out", Value::of(17_u64))),
            Action::Yield => Step::Yield,
            Action::WaitInput => Step::WaitInput("next".into()),
            Action::WaitInputs => Step::WaitInputs(Readiness::Any),
            Action::WaitWake => Step::WaitWake,
            Action::Complete => Step::Complete,
            Action::Finished => Step::Finished,
            Action::Fail => {
                return Err(Failure::external(
                    "scripted compact failure",
                    CompactSource { marker: 0xc04d },
                ));
            }
        })
    }
}
impl Drop for Scripted {
    fn drop(&mut self) {
        assert!(self.probes.lease.upgrade().is_some());
        self.probes.process_drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct UnreachableProcess;
impl Process for UnreachableProcess {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        unreachable!("placeholder handoff must not execute")
    }
}

struct ContextProbe(Probes);
impl Drop for ContextProbe {
    fn drop(&mut self) {
        assert!(self.0.lease.upgrade().is_some());
        self.0.context_drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct Probes {
    effects: std::sync::Arc<AtomicUsize>,
    process_drops: std::sync::Arc<AtomicUsize>,
    context_drops: std::sync::Arc<AtomicUsize>,
    lease: Weak<()>,
}
impl Probes {
    fn new() -> (Self, std::sync::Arc<()>) {
        let lease = std::sync::Arc::new(());
        (
            Self {
                effects: std::sync::Arc::new(AtomicUsize::new(0)),
                process_drops: std::sync::Arc::new(AtomicUsize::new(0)),
                context_drops: std::sync::Arc::new(AtomicUsize::new(0)),
                lease: std::sync::Arc::downgrade(&lease),
            },
            lease,
        )
    }
}

fn scripted(script: Script) -> (Box<dyn Process>, Probes, std::sync::Arc<()>) {
    let (probes, lease) = Probes::new();
    let process = Box::new(Scripted {
        script,
        position: 0,
        probes: probes.clone(),
    });
    (process, probes, lease)
}

struct Retired {
    #[allow(dead_code)]
    job: ActivationJob,
    effects: std::sync::Arc<AtomicUsize>,
    process_drops: std::sync::Arc<AtomicUsize>,
}

enum Runner {
    Job(ActivationJob),
    Completion(CompletedStep),
}
enum Settled {
    Step(Step),
    Emission,
    Failure(Failure),
}
struct Real {
    handoff: ProcessHandoff,
    runner: Option<Runner>,
    emission: Option<ProcessEmission>,
    probes: Probes,
    retired: Option<Retired>,
}
impl Real {
    fn new(script: Script) -> Self {
        let (process, probes, lease) = scripted(script);
        let handoff = ProcessHandoff::new(process).with_lease(lease);
        Self {
            handoff,
            runner: None,
            emission: None,
            probes,
            retired: None,
        }
    }

    fn admit(&mut self) {
        self.handoff
            .admit(ProcessContext::new(
                [(
                    "initial".into(),
                    Value::local(ContextProbe(Probes {
                        effects: self.probes.effects.clone(),
                        process_drops: self.probes.process_drops.clone(),
                        context_drops: self.probes.context_drops.clone(),
                        lease: self.probes.lease.clone(),
                    })),
                )]
                .into(),
                vec![],
            ))
            .unwrap();
    }

    fn dispatch(&mut self) {
        assert!(self.runner.is_none());
        self.runner = Some(Runner::Job(self.handoff.next_job().unwrap()));
    }

    fn run(&mut self) {
        let Some(Runner::Job(job)) = self.runner.take() else {
            panic!("anchor requires an unrun real job")
        };
        self.runner = Some(Runner::Completion(job.run()));
    }

    fn reject(&mut self) {
        let Some(Runner::Job(job)) = self.runner.take() else {
            panic!("anchor requires an unrun real job")
        };
        self.handoff.reject(job).unwrap().unwrap();
    }

    fn take_job(&mut self) -> ActivationJob {
        let Some(Runner::Job(job)) = self.runner.take() else {
            panic!("matrix requires an unrun real job")
        };
        job
    }

    fn take_completion(&mut self) -> CompletedStep {
        let Some(Runner::Completion(completion)) = self.runner.take() else {
            panic!("matrix requires a real completed step")
        };
        completion
    }

    fn settle(&mut self) -> Settled {
        let completion = self.take_completion();
        match self.handoff.complete(completion).unwrap() {
            Ok(Step::Emit(emission)) => {
                self.emission = Some(emission);
                Settled::Emission
            }
            Ok(step) => Settled::Step(step),
            Err(failure) => Settled::Failure(failure),
        }
    }

    fn assert_matches(&self, anchor: Anchor, model: &Oracle) {
        assert_eq!(
            self.handoff.state(),
            model.state,
            "{anchor:?} state after {}",
            model.provenance
        );
        assert_eq!(
            self.probes.effects.load(Ordering::SeqCst),
            model.effects,
            "{anchor:?} process effects"
        );
        assert_eq!(
            self.probes.process_drops.load(Ordering::SeqCst),
            model.process_drops,
            "{anchor:?} process cleanup"
        );
        assert_eq!(
            self.probes.context_drops.load(Ordering::SeqCst),
            model.context_drops,
            "{anchor:?} context cleanup"
        );
        assert_eq!(
            self.probes.lease.upgrade().is_some(),
            model.lease_alive,
            "{anchor:?} externally visible lease"
        );
        assert_eq!(
            self.runner_kind(),
            model.runner,
            "{anchor:?} object location"
        );
        assert_eq!(
            self.emission.is_some(),
            model.emission,
            "{anchor:?} emission location"
        );
        match model.outstanding {
            Some((activation, sequence)) => {
                let ticket = self.handoff.outstanding().expect("real outstanding ticket");
                assert_eq!(ticket.activation(), activation);
                assert_eq!(ticket.sequence(), sequence);
            }
            None => assert_eq!(self.handoff.outstanding(), None),
        }
        if let Some(retired) = &self.retired {
            assert_eq!(retired.effects.load(Ordering::SeqCst), 0);
            assert_eq!(retired.process_drops.load(Ordering::SeqCst), 0);
        }
    }

    fn runner_kind(&self) -> RunnerKind {
        match &self.runner {
            None => RunnerKind::None,
            Some(Runner::Job(_)) => RunnerKind::Job,
            Some(Runner::Completion(_)) => RunnerKind::Completion,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunnerKind {
    None,
    Job,
    Completion,
}

struct Oracle {
    state: HandoffState,
    provenance: &'static str,
    activation: u64,
    sequence: u64,
    effects: usize,
    outstanding: Option<(u64, u64)>,
    runner: RunnerKind,
    emission: bool,
    process_drops: usize,
    context_live: bool,
    context_drops: usize,
    lease_alive: bool,
    cleaned: bool,
}
impl Oracle {
    fn new(state: HandoffState) -> Self {
        Self {
            state,
            provenance: "documented operation envelope",
            activation: 0,
            sequence: 0,
            effects: 0,
            outstanding: None,
            runner: RunnerKind::None,
            emission: false,
            process_drops: 0,
            context_live: false,
            context_drops: 0,
            lease_alive: true,
            cleaned: false,
        }
    }

    fn admit(&mut self) {
        self.activation += 1;
        self.state = HandoffState::Ready;
        self.context_live = true;
    }

    fn dispatch(&mut self) {
        self.sequence += 1;
        self.state = HandoffState::Dispatched;
        self.outstanding = Some((self.activation, self.sequence));
        self.runner = RunnerKind::Job;
    }

    fn settle(&mut self, state: HandoffState, emission: bool) {
        self.state = state;
        self.outstanding = None;
        self.runner = RunnerKind::None;
        self.emission = emission;
    }

    fn complete_activation(&mut self) {
        assert!(self.context_live);
        self.context_live = false;
        self.context_drops += 1;
    }

    fn clean_up(&mut self) {
        if self.cleaned || self.runner != RunnerKind::None {
            return;
        }
        self.cleaned = true;
        self.process_drops += 1;
        if self.context_live {
            self.context_live = false;
            self.context_drops += 1;
        }
        if !self.emission {
            self.lease_alive = false;
        }
    }

    fn apply(&mut self, real: &mut Real, operation: Operation) -> Attempt {
        match operation {
            Operation::Admit => {
                if self.state == HandoffState::Idle {
                    real.admit();
                    self.admit();
                    Attempt::Accepted
                } else {
                    let expected = self.state;
                    let (actual, returned) = real.handoff.admit(additional_context()).unwrap_err();
                    assert_eq!(actual, HandoffError::InvalidState(expected));
                    assert!(returned.input("next").is_some());
                    Attempt::Refused
                }
            }
            Operation::NextJob => {
                if self.state == HandoffState::Ready {
                    self.sequence += 1;
                    let job = real.handoff.next_job().unwrap();
                    assert_eq!(job.ticket().activation(), self.activation);
                    assert_eq!(job.ticket().sequence(), self.sequence);
                    assert!(real.runner.is_none());
                    real.runner = Some(Runner::Job(job));
                    self.state = HandoffState::Dispatched;
                    self.outstanding = Some((self.activation, self.sequence));
                    self.runner = RunnerKind::Job;
                    Attempt::Accepted
                } else {
                    let expected = self.state;
                    assert_eq!(
                        real.handoff.next_job().unwrap_err(),
                        HandoffError::InvalidState(expected)
                    );
                    Attempt::Refused
                }
            }
            Operation::ResumeInput => {
                if self.state == HandoffState::WaitingInput {
                    real.handoff.resume_input(additional_context()).unwrap();
                    self.state = HandoffState::Ready;
                    Attempt::Accepted
                } else {
                    let expected = self.state;
                    let (actual, returned) =
                        real.handoff.resume_input(additional_context()).unwrap_err();
                    assert_eq!(actual, HandoffError::InvalidState(expected));
                    assert!(returned.input("next").is_some());
                    Attempt::Refused
                }
            }
            Operation::ResumeWake => {
                if self.state == HandoffState::WaitingWake {
                    real.handoff.resume_wake().unwrap();
                    self.state = HandoffState::Ready;
                    Attempt::Accepted
                } else {
                    let expected = self.state;
                    assert_eq!(
                        real.handoff.resume_wake().unwrap_err(),
                        HandoffError::InvalidState(expected)
                    );
                    Attempt::Refused
                }
            }
            Operation::PublicationCommitted => {
                if self.state == HandoffState::PublicationPending {
                    real.handoff.publication_committed().unwrap();
                    self.state = HandoffState::Ready;
                    Attempt::Accepted
                } else {
                    let expected = self.state;
                    assert_eq!(
                        real.handoff.publication_committed().unwrap_err(),
                        HandoffError::InvalidState(expected)
                    );
                    Attempt::Refused
                }
            }
            Operation::PublicationFailed => {
                if self.state == HandoffState::PublicationPending {
                    real.handoff.publication_failed().unwrap();
                    self.state = HandoffState::Failed;
                    self.clean_up();
                    Attempt::Accepted
                } else {
                    let expected = self.state;
                    assert_eq!(
                        real.handoff.publication_failed().unwrap_err(),
                        HandoffError::InvalidState(expected)
                    );
                    Attempt::Refused
                }
            }
            Operation::FinishIdle => {
                if self.state == HandoffState::Idle {
                    real.handoff.finish_idle().unwrap();
                    self.state = HandoffState::Finished;
                    self.clean_up();
                    Attempt::Accepted
                } else {
                    let expected = self.state;
                    assert_eq!(
                        real.handoff.finish_idle().unwrap_err(),
                        HandoffError::InvalidState(expected)
                    );
                    Attempt::Refused
                }
            }
            Operation::Cancel => {
                let expected_inventory = if self.state == HandoffState::Cancelled {
                    None
                } else {
                    self.outstanding
                };
                let actual = real.handoff.cancel().unwrap();
                match (expected_inventory, actual) {
                    (None, None) => {}
                    (Some((activation, sequence)), Some(ticket)) => {
                        assert_eq!(ticket.activation(), activation);
                        assert_eq!(ticket.sequence(), sequence);
                    }
                    pair => panic!("cancel inventory mismatch: {pair:?}"),
                }
                self.state = HandoffState::Cancelled;
                self.outstanding = None;
                self.clean_up();
                Attempt::Accepted
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Admit,
    NextJob,
    ResumeInput,
    ResumeWake,
    PublicationCommitted,
    PublicationFailed,
    FinishIdle,
    Cancel,
}
impl Operation {
    const ALL: [Self; 8] = [
        Self::Admit,
        Self::NextJob,
        Self::ResumeInput,
        Self::ResumeWake,
        Self::PublicationCommitted,
        Self::PublicationFailed,
        Self::FinishIdle,
        Self::Cancel,
    ];
    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap()
    }

    /// Required source state in the independently documented transition table.
    /// `None` means that every source state accepts the operation (cancellation).
    fn required_state(self) -> Option<HandoffState> {
        match self {
            Self::Admit | Self::FinishIdle => Some(HandoffState::Idle),
            Self::NextJob => Some(HandoffState::Ready),
            Self::ResumeInput => Some(HandoffState::WaitingInput),
            Self::ResumeWake => Some(HandoffState::WaitingWake),
            Self::PublicationCommitted | Self::PublicationFailed => {
                Some(HandoffState::PublicationPending)
            }
            Self::Cancel => None,
        }
    }

    /// `None` is an invalid attempt and therefore leaves the source unchanged.
    fn transition(self, source: HandoffState) -> Option<HandoffState> {
        if self
            .required_state()
            .is_some_and(|required| required != source)
        {
            return None;
        }
        Some(match self {
            Self::Admit | Self::ResumeInput | Self::ResumeWake | Self::PublicationCommitted => {
                HandoffState::Ready
            }
            Self::NextJob => HandoffState::Dispatched,
            Self::PublicationFailed => HandoffState::Failed,
            Self::FinishIdle => HandoffState::Finished,
            Self::Cancel => HandoffState::Cancelled,
        })
    }
}

fn expected_suffix_distribution(anchor_states: &[usize; 9]) -> [(usize, usize); 8] {
    let states = [
        HandoffState::Idle,
        HandoffState::Ready,
        HandoffState::Dispatched,
        HandoffState::PublicationPending,
        HandoffState::WaitingInput,
        HandoffState::WaitingWake,
        HandoffState::Finished,
        HandoffState::Failed,
        HandoffState::Cancelled,
    ];
    Operation::ALL.map(|operation| {
        let mut accepted = 0;
        let mut attempts = 0;
        for (source, anchors) in states.into_iter().zip(anchor_states) {
            // A valid singleton occurs once, and the same valid first operation
            // occurs with all eight possible second operations.
            if operation.transition(source).is_some() {
                accepted += 9 * anchors;
            }
            // For pair-second acceptance, independently evaluate every possible
            // predecessor without executing or inspecting a real handoff.
            for first in Operation::ALL {
                let after_first = first.transition(source).unwrap_or(source);
                let eligible = operation
                    .required_state()
                    .is_none_or(|required| required == after_first);
                if eligible {
                    accepted += anchors;
                }
            }
            attempts += 17 * anchors;
        }
        (accepted, attempts - accepted)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attempt {
    Accepted,
    Refused,
}

fn state_index(state: HandoffState) -> usize {
    match state {
        HandoffState::Idle => 0,
        HandoffState::Ready => 1,
        HandoffState::Dispatched => 2,
        HandoffState::PublicationPending => 3,
        HandoffState::WaitingInput => 4,
        HandoffState::WaitingWake => 5,
        HandoffState::Finished => 6,
        HandoffState::Failed => 7,
        HandoffState::Cancelled => 8,
    }
}
#[derive(Default)]
struct AttemptCount {
    accepted: usize,
    refused: usize,
}
impl AttemptCount {
    fn record(&mut self, attempt: Attempt) {
        match attempt {
            Attempt::Accepted => self.accepted += 1,
            Attempt::Refused => self.refused += 1,
        }
    }
}

fn additional_context() -> ProcessContext {
    ProcessContext::new([("next".into(), Value::of(23_u64))].into(), vec![])
}

#[test]
fn bounded_real_handoff_suffix_envelope() {
    let mut suffixes = vec![vec![]];
    suffixes.extend(Operation::ALL.into_iter().map(|operation| vec![operation]));
    for first in Operation::ALL {
        for second in Operation::ALL {
            suffixes.push(vec![first, second]);
        }
    }
    assert_eq!(suffixes.len(), SUFFIXES_PER_ANCHOR);
    let mut histories = 0;
    let mut attempts = 0;
    let mut anchor_states = [0_usize; 9];
    let mut by_operation: [AttemptCount; 8] = std::array::from_fn(|_| AttemptCount::default());
    for anchor in Anchor::ALL {
        for suffix in &suffixes {
            let (mut real, mut model) = anchor.build();
            real.assert_matches(anchor, &model);
            if suffix.is_empty() {
                anchor_states[state_index(model.state)] += 1;
            }
            for operation in suffix {
                let attempt = model.apply(&mut real, *operation);
                real.assert_matches(anchor, &model);
                by_operation[operation.index()].record(attempt);
                attempts += 1;
            }
            histories += 1;
        }
    }
    assert_eq!(histories, HISTORIES);
    assert_eq!(attempts, OPERATION_ATTEMPTS);
    assert_eq!(anchor_states, [3, 6, 2, 1, 2, 1, 2, 2, 2]);
    let expected = expected_suffix_distribution(&anchor_states);
    for ((operation, count), (accepted, refused)) in
        Operation::ALL.into_iter().zip(&by_operation).zip(expected)
    {
        assert_eq!(count.accepted + count.refused, OPERATION_ATTEMPTS / 8);
        assert_eq!(
            (count.accepted, count.refused),
            (accepted, refused),
            "{operation:?}"
        );
    }
    let expected_accepted: usize = expected.iter().map(|count| count.0).sum();
    let expected_refused: usize = expected.iter().map(|count| count.1).sum();
    let accepted: usize = by_operation.iter().map(|count| count.accepted).sum();
    let refused: usize = by_operation.iter().map(|count| count.refused).sum();
    assert_eq!((accepted, refused), (expected_accepted, expected_refused));
    eprintln!(
        "Q5 real handoff envelope: anchors={ANCHORS} suffixes_per_anchor={SUFFIXES_PER_ANCHOR} histories={histories} operations={attempts} accepted={accepted} refused={refused}"
    );
    eprintln!(
        "Q5 anchor states: Idle={} Ready={} Dispatched={} PublicationPending={} WaitingInput={} WaitingWake={} Finished={} Failed={} Cancelled={}",
        anchor_states[0],
        anchor_states[1],
        anchor_states[2],
        anchor_states[3],
        anchor_states[4],
        anchor_states[5],
        anchor_states[6],
        anchor_states[7],
        anchor_states[8]
    );
    for (operation, count) in Operation::ALL.into_iter().zip(by_operation) {
        eprintln!(
            "Q5 handoff {operation:?}: attempts={} accepted={} refused={}",
            count.accepted + count.refused,
            count.accepted,
            count.refused
        );
    }
}

#[test]
fn replacement_anchor_has_fresh_authority_and_monotonic_ticket_accounting() {
    let (mut real, _) = Anchor::ReplacementFresh.build();
    let retired = real.retired.take().unwrap();
    let old_ticket = retired.job.ticket();
    // The replacement is a different owner for unrun rejection too. Move the
    // same returned job into one real run rather than cloning or rebuilding it.
    let (error, old_job) = real.handoff.reject(retired.job).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    assert_eq!(old_job.ticket(), old_ticket);
    assert_eq!(retired.effects.load(Ordering::SeqCst), 0);
    assert_eq!(real.handoff.state(), HandoffState::Idle);
    let old_completion = old_job.run();
    assert_eq!(retired.effects.load(Ordering::SeqCst), 1);
    let (error, returned) = real.handoff.complete(old_completion).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    assert_eq!(returned.ticket(), old_ticket);
    assert_eq!(real.handoff.state(), HandoffState::Idle);
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 0);
    assert_eq!(retired.process_drops.load(Ordering::SeqCst), 0);

    real.admit();
    let fresh = real.handoff.next_job().unwrap();
    assert_eq!(fresh.ticket().activation(), old_ticket.activation() + 1);
    assert_eq!(fresh.ticket().sequence(), old_ticket.sequence() + 1);
    assert!(matches!(
        real.handoff.complete(fresh.run()).unwrap(),
        Ok(Step::Complete)
    ));
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
    drop(returned);
    assert_eq!(retired.process_drops.load(Ordering::SeqCst), 1);
}

#[test]
fn rejected_input_duplicates_return_the_new_owned_context() {
    let (mut real, _) = Anchor::WaitInput.build();
    let (error, returned) = real
        .handoff
        .resume_input(ProcessContext::new(
            [("initial".into(), Value::of(23_u64))].into(),
            vec![],
        ))
        .unwrap_err();
    assert_eq!(
        error,
        HandoffError::InvalidState(HandoffState::WaitingInput)
    );
    assert_eq!(returned.input("initial").unwrap().get::<u64>().unwrap(), 23);
    assert_eq!(real.handoff.state(), HandoffState::WaitingInput);
    real.handoff.resume_input(additional_context()).unwrap();
    assert_eq!(real.handoff.state(), HandoffState::Ready);
}

fn complete_tail(
    real: &mut Real,
    previous_ticket: StepTicket,
    new_activation: bool,
    expected_context_drops: usize,
) {
    real.dispatch();
    let ticket = real.handoff.outstanding().expect("tail ticket");
    assert_eq!(
        ticket.activation(),
        previous_ticket.activation() + u64::from(new_activation)
    );
    assert_eq!(ticket.sequence(), previous_ticket.sequence() + 1);
    real.run();
    assert!(matches!(real.settle(), Settled::Step(Step::Complete)));
    assert_eq!(real.handoff.state(), HandoffState::Idle);
    assert_eq!(real.handoff.outstanding(), None);
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 2);
    assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        real.probes.context_drops.load(Ordering::SeqCst),
        expected_context_drops
    );
}

fn settle_accepted_outcome_and_continue(
    real: &mut Real,
    action: Action,
    subject_ticket: StepTicket,
) {
    let settled = real.settle();
    assert_eq!(real.handoff.outstanding(), None);
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
    match action {
        Action::Emit => {
            assert!(matches!(settled, Settled::Emission));
            assert_eq!(real.handoff.state(), HandoffState::PublicationPending);
            let emission = real.emission.take().expect("retained real emission");
            let mut connections = crate::connection::ConnectionSet::new(vec![
                crate::connection::ConnectionQueue::new(
                    NonZeroUsize::new(1).unwrap(),
                    Value::of(0_u64).identity(),
                ),
            ]);
            let targets = [crate::connection::FanoutTarget::identity(0)];
            let packet = crate::connection::Packet::new(emission.value);
            let mut transaction = connections.prepare(&packet, &targets).unwrap();
            connections.commit(&mut transaction).unwrap();
            assert_eq!(
                connections.commit(&mut transaction),
                Err(crate::connection::ConnectionError::AlreadyCommitted)
            );
            let packet = connections.pop(0).expect("exact accepted packet");
            assert_eq!(*packet.value().decoded::<u64>().unwrap(), 17);
            assert!(connections.pop(0).is_none());
            real.handoff.publication_committed().unwrap();
            complete_tail(real, subject_ticket, false, 1);
        }
        Action::Yield => {
            assert!(matches!(settled, Settled::Step(Step::Yield)));
            assert_eq!(real.handoff.state(), HandoffState::Ready);
            complete_tail(real, subject_ticket, false, 1);
        }
        Action::WaitInput | Action::WaitInputs => {
            if action == Action::WaitInput {
                assert!(matches!(
                    settled,
                    Settled::Step(Step::WaitInput(name)) if name == "next"
                ));
            } else {
                assert!(matches!(settled, Settled::Step(Step::WaitInputs(_))));
            }
            assert_eq!(real.handoff.state(), HandoffState::WaitingInput);
            // This is intentionally only a handoff continuation. Built-in group
            // selection and EOS semantics remain runtime-composition coverage.
            real.handoff.resume_input(additional_context()).unwrap();
            complete_tail(real, subject_ticket, false, 1);
        }
        Action::WaitWake => {
            assert!(matches!(settled, Settled::Step(Step::WaitWake)));
            assert_eq!(real.handoff.state(), HandoffState::WaitingWake);
            real.handoff.resume_wake().unwrap();
            complete_tail(real, subject_ticket, false, 1);
        }
        Action::Complete => {
            assert!(matches!(settled, Settled::Step(Step::Complete)));
            assert_eq!(real.handoff.state(), HandoffState::Idle);
            assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 0);
            assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 1);
            real.admit();
            complete_tail(real, subject_ticket, true, 2);
        }
        Action::Finished => {
            assert!(matches!(settled, Settled::Step(Step::Finished)));
            assert_eq!(real.handoff.state(), HandoffState::Finished);
            assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 1);
            assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 1);
            assert_eq!(
                real.handoff.next_job().unwrap_err(),
                HandoffError::InvalidState(HandoffState::Finished)
            );
        }
        Action::Fail => {
            let Settled::Failure(failure) = settled else {
                panic!("scripted failure was not accepted as an inner Failure")
            };
            assert_eq!(real.handoff.state(), HandoffState::Failed);
            assert_eq!(failure.context().ticket, Some(subject_ticket));
            assert_eq!(failure.context().phase, Some(CallbackPhase::Step));
            assert_eq!(
                failure
                    .source()
                    .and_then(|source| source.downcast_ref::<CompactSource>())
                    .map(|source| source.marker),
                Some(0xc04d)
            );
            assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 1);
            assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 1);
            assert_eq!(
                real.handoff.next_job().unwrap_err(),
                HandoffError::InvalidState(HandoffState::Failed)
            );
        }
    }
}

#[test]
fn rejected_redispatch_foreign_returns_and_accepted_completion_cover_every_outcome() {
    let mut outcomes = 0;
    for action in Action::ALL {
        let mut real = Real::new(action.continued_script());
        real.admit();
        real.dispatch();
        let rejected_ticket = real.handoff.outstanding().unwrap();
        real.reject();
        assert_eq!(real.handoff.state(), HandoffState::Ready);
        assert_eq!(real.handoff.outstanding(), None);
        assert_eq!(real.probes.effects.load(Ordering::SeqCst), 0);
        assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 0);

        real.dispatch();
        let dispatched_ticket = real.handoff.outstanding().unwrap();
        assert_ne!(dispatched_ticket, rejected_ticket);
        assert_eq!(dispatched_ticket.activation(), rejected_ticket.activation());
        assert_eq!(dispatched_ticket.sequence(), rejected_ticket.sequence() + 1);

        // A foreign owner cannot reject and thereby destroy this fresh job.
        let mut foreign = ProcessHandoff::new(Box::new(UnreachableProcess));
        let (error, job) = foreign.reject(real.take_job()).unwrap_err();
        assert_eq!(error, HandoffError::ForeignCompletion);
        assert_eq!(job.ticket(), dispatched_ticket);
        assert_eq!(foreign.state(), HandoffState::Idle);
        assert_eq!(foreign.outstanding(), None);
        assert_eq!(real.handoff.outstanding(), Some(dispatched_ticket));
        assert_eq!(real.probes.effects.load(Ordering::SeqCst), 0);
        real.runner = Some(Runner::Job(job));

        real.run();
        assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
        // Nor can the same foreign owner consume the real CompletedStep.
        let (error, completion) = foreign.complete(real.take_completion()).unwrap_err();
        assert_eq!(error, HandoffError::ForeignCompletion);
        assert_eq!(completion.ticket(), dispatched_ticket);
        assert_eq!(foreign.state(), HandoffState::Idle);
        assert_eq!(foreign.outstanding(), None);
        assert_eq!(real.handoff.state(), HandoffState::Dispatched);
        assert_eq!(real.handoff.outstanding(), Some(dispatched_ticket));
        real.runner = Some(Runner::Completion(*completion));

        // The one rightful completion is accepted, then its actual continuation
        // runs exactly once whenever the outcome leaves a live process.
        settle_accepted_outcome_and_continue(&mut real, action, dispatched_ticket);
        outcomes += 1;
    }
    assert_eq!(outcomes, Action::ALL.len());
    eprintln!(
        "Q5 active outcome matrix: outcomes={outcomes} rightful_rejections={outcomes} fresh_dispatches={outcomes} foreign_reject_returns={outcomes} foreign_completion_returns={outcomes} accepted_completions={outcomes}"
    );
}

#[test]
fn stale_completion_matrix_preserves_every_real_result_until_return_drop() {
    let mut outcomes = 0;
    for action in Action::ALL {
        let mut real = Real::new(action.continued_script());
        real.admit();
        real.dispatch();
        let ticket = real.handoff.outstanding().unwrap();
        real.run();
        assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
        assert_eq!(real.handoff.cancel().unwrap(), Some(ticket));
        assert_eq!(real.handoff.state(), HandoffState::Cancelled);
        assert_eq!(real.handoff.outstanding(), None);

        let completion = real.take_completion();
        let (error, returned) = real.handoff.complete(completion).unwrap_err();
        assert_eq!(error, HandoffError::StaleCompletion);
        assert_eq!(returned.ticket(), ticket);
        assert_eq!(real.handoff.state(), HandoffState::Cancelled);
        assert_eq!(real.handoff.outstanding(), None);
        assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
        assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 0);
        assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 0);
        assert!(real.probes.lease.upgrade().is_some());

        // Result payload, process, context, and source/code lease all remain
        // attached to this one returned object until its actual destruction.
        drop(returned);
        assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 1);
        assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 1);
        assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
        assert!(real.probes.lease.upgrade().is_none());
        outcomes += 1;
    }
    assert_eq!(outcomes, Action::ALL.len());
    eprintln!(
        "Q5 stale outcome matrix: outcomes={outcomes} cancelled_dispatches={outcomes} intact_stale_returns={outcomes} exact_return_drop_cleanups={outcomes}"
    );
}

#[test]
fn cancelled_unrun_rejection_returns_the_same_job_for_one_real_run() {
    let mut real = Real::new(vec![Action::Complete]);
    real.admit();
    real.dispatch();
    let ticket = real.handoff.outstanding().unwrap();
    let job = real.take_job();
    assert_eq!(real.handoff.cancel().unwrap(), Some(ticket));
    let (error, job) = real.handoff.reject(job).unwrap_err();
    assert_eq!(error, HandoffError::StaleCompletion);
    assert_eq!(job.ticket(), ticket);
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 0);
    assert_eq!(real.handoff.outstanding(), None);

    real.runner = Some(Runner::Job(job));
    real.run();
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
    let completion = real.take_completion();
    let (error, returned) = real.handoff.complete(completion).unwrap_err();
    assert_eq!(error, HandoffError::StaleCompletion);
    assert_eq!(returned.ticket(), ticket);
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
    drop(returned);
    assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 1);
    assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 1);
}

#[test]
fn post_complete_idle_reentry_cleans_each_context_but_preserves_process() {
    let (mut real, model) = Anchor::PostCompleteIdle.build();
    assert_eq!(real.handoff.state(), HandoffState::Idle);
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 1);
    assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 1);
    assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 0);

    real.admit();
    real.dispatch();
    let ticket = real.handoff.outstanding().unwrap();
    assert_eq!(ticket.activation(), model.activation + 1);
    assert_eq!(ticket.sequence(), model.sequence + 1);
    real.run();
    assert!(matches!(real.settle(), Settled::Step(Step::Complete)));
    assert_eq!(real.handoff.state(), HandoffState::Idle);
    assert_eq!(real.probes.effects.load(Ordering::SeqCst), 2);
    assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 2);
    assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 0);

    real.handoff.finish_idle().unwrap();
    assert_eq!(real.handoff.state(), HandoffState::Finished);
    assert_eq!(real.probes.process_drops.load(Ordering::SeqCst), 1);
    assert_eq!(real.probes.context_drops.load(Ordering::SeqCst), 2);
    assert!(real.probes.lease.upgrade().is_none());
}
