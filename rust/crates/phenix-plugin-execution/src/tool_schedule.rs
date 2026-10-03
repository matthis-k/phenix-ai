use std::num::NonZeroUsize;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToolConcurrency {
    ParallelSafe,
    #[default]
    Exclusive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallPlan {
    pub id: String,
    pub concurrency: ToolConcurrency,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScheduledToolBatch {
    Parallel(Vec<ToolCallPlan>),
    Exclusive(ToolCallPlan),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToolScheduler {
    max_parallel_calls: Option<NonZeroUsize>,
}

impl ToolScheduler {
    #[must_use]
    pub const fn new(max_parallel_calls: NonZeroUsize) -> Self {
        Self {
            max_parallel_calls: Some(max_parallel_calls),
        }
    }

    #[must_use]
    pub const fn max_parallel_calls(self) -> Option<NonZeroUsize> {
        self.max_parallel_calls
    }

    #[must_use]
    pub fn schedule(
        self,
        calls: impl IntoIterator<Item = ToolCallPlan>,
    ) -> Vec<ScheduledToolBatch> {
        let mut batches = Vec::new();
        let mut parallel = Vec::new();

        for call in calls {
            match call.concurrency {
                ToolConcurrency::ParallelSafe => {
                    parallel.push(call);
                    if self
                        .max_parallel_calls
                        .is_some_and(|limit| parallel.len() == limit.get())
                    {
                        batches.push(ScheduledToolBatch::Parallel(std::mem::take(&mut parallel)));
                    }
                }
                ToolConcurrency::Exclusive => {
                    flush_parallel(&mut batches, &mut parallel);
                    batches.push(ScheduledToolBatch::Exclusive(call));
                }
            }
        }
        flush_parallel(&mut batches, &mut parallel);
        batches
    }
}


fn flush_parallel(batches: &mut Vec<ScheduledToolBatch>, parallel: &mut Vec<ToolCallPlan>) {
    if parallel.is_empty() {
        return;
    }
    batches.push(ScheduledToolBatch::Parallel(std::mem::take(parallel)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, concurrency: ToolConcurrency) -> ToolCallPlan {
        ToolCallPlan {
            id: id.into(),
            concurrency,
        }
    }

    #[test]
    fn default_scheduler_has_no_parallel_limit() {
        assert_eq!(ToolScheduler::default().max_parallel_calls(), None);
    }

    #[test]
    fn tools_are_exclusive_unless_the_call_is_declared_parallel_safe() {
        assert_eq!(ToolConcurrency::default(), ToolConcurrency::Exclusive);
    }

    #[test]
    fn exclusive_calls_are_barriers_between_parallel_safe_groups() {
        let scheduled = ToolScheduler::default().schedule([
            call("read-a", ToolConcurrency::ParallelSafe),
            call("read-b", ToolConcurrency::ParallelSafe),
            call("write", ToolConcurrency::Exclusive),
            call("read-c", ToolConcurrency::ParallelSafe),
        ]);

        assert_eq!(
            scheduled,
            vec![
                ScheduledToolBatch::Parallel(vec![
                    call("read-a", ToolConcurrency::ParallelSafe),
                    call("read-b", ToolConcurrency::ParallelSafe),
                ]),
                ScheduledToolBatch::Exclusive(call("write", ToolConcurrency::Exclusive)),
                ScheduledToolBatch::Parallel(vec![call("read-c", ToolConcurrency::ParallelSafe,)]),
            ]
        );
    }

    #[test]
    fn parallel_groups_never_exceed_the_scheduler_cap() {
        let scheduler = ToolScheduler::new(NonZeroUsize::new(2).unwrap());
        let scheduled = scheduler.schedule([
            call("a", ToolConcurrency::ParallelSafe),
            call("b", ToolConcurrency::ParallelSafe),
            call("c", ToolConcurrency::ParallelSafe),
        ]);

        assert_eq!(
            scheduled,
            vec![
                ScheduledToolBatch::Parallel(vec![
                    call("a", ToolConcurrency::ParallelSafe),
                    call("b", ToolConcurrency::ParallelSafe),
                ]),
                ScheduledToolBatch::Parallel(vec![call("c", ToolConcurrency::ParallelSafe)]),
            ]
        );
    }
}
