//! Owned worker handoff and bounded publication primitives. This example does
//! not claim to be the canonical network builder/session migration.
use phenix_graph::Value;
use phenix_graph::connection::{ConnectionQueue, ConnectionSet, FanoutTarget, Packet};
use phenix_graph::process::{
    ActivationJob, DispatchResult, Process, ProcessContext, ProcessDispatcher, ProcessEmission,
    ProcessHandoff, Step,
};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::mpsc;

struct Sum {
    total: i64,
    emitted: bool,
}
impl Process for Sum {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        self.total += context
            .take("increment")
            .ok_or("missing increment")?
            .get::<i64>()?;
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "total",
            Value::of(self.total),
        )))
    }
}

struct WorkerDispatcher(mpsc::SyncSender<ActivationJob>);
impl ProcessDispatcher for WorkerDispatcher {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        match self.0.try_send(job) {
            Ok(()) => DispatchResult::Deferred,
            Err(mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job)) => {
                DispatchResult::Rejected(job)
            }
        }
    }
}

fn main() {
    let (jobs, received_jobs) = mpsc::sync_channel::<ActivationJob>(1);
    let (completions, received_completions) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        for job in received_jobs {
            // Host-specific worker guards belong here, around delegated code.
            if completions.send(job.run()).is_err() {
                break;
            }
        }
    });
    let dispatcher = WorkerDispatcher(jobs);
    let mut process = ProcessHandoff::new(Box::new(Sum {
        total: 0,
        emitted: false,
    }));
    let mut transport = ConnectionSet::new(vec![ConnectionQueue::new(
        NonZeroUsize::new(1).unwrap(),
        Value::of(0_i64).identity(),
    )]);
    for (increment, expected) in [(2_i64, 2_i64), (3, 5)] {
        process
            .admit(ProcessContext::new(
                BTreeMap::from([("increment".into(), Value::of(increment))]),
                vec![],
            ))
            .unwrap();
        assert!(matches!(
            dispatcher.dispatch(process.next_job().unwrap()),
            DispatchResult::Deferred
        ));
        let completion = received_completions.recv().unwrap();
        let Step::Emit(emission) = process.complete(completion).unwrap().unwrap() else {
            panic!("sum did not emit");
        };
        assert_eq!(emission.output, "total");
        let mut publication = transport
            .prepare(&Packet::new(emission.value), &[FanoutTarget::identity(0)])
            .unwrap();
        transport.commit(&mut publication).unwrap();
        process.publication_committed().unwrap();
        assert!(matches!(
            dispatcher.dispatch(process.next_job().unwrap()),
            DispatchResult::Deferred
        ));
        assert!(matches!(
            process
                .complete(received_completions.recv().unwrap())
                .unwrap()
                .unwrap(),
            Step::Complete
        ));
        assert_eq!(
            transport.pop(0).unwrap().value().get::<i64>().unwrap(),
            expected
        );
    }
    transport.close_writer(0).unwrap();
    assert!(transport.queues()[0].is_eos());
    drop(dispatcher);
    worker.join().unwrap();
}
