use crate::{Authority, EventBus, GraphGenerationId, KernelEvent, PluginId};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex, Weak,
    },
    thread::{self, JoinHandle},
};

#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    authority: Authority,
    graph_generation: GraphGenerationId,
}

impl CancellationToken {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    pub fn graph_generation(&self) -> &GraphGenerationId {
        &self.graph_generation
    }
}

#[derive(Clone, Debug)]
pub struct CallCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CallCancellationToken {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

pub(crate) struct LiveCallScope<'a> {
    runtime: &'a TaskRuntime,
    plugin: PluginId,
    id: u64,
    cancellation: CallCancellationToken,
}

impl LiveCallScope<'_> {
    #[cfg(test)]
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn cancellation_token(&self) -> &CallCancellationToken {
        &self.cancellation
    }
}

impl Drop for LiveCallScope<'_> {
    fn drop(&mut self) {
        self.runtime.finish_call(&self.plugin, self.id);
    }
}

pub struct TaskHandle<T> {
    id: u64,
    graph_generation: GraphGenerationId,
    cancelled: Arc<AtomicBool>,
    receiver: Receiver<T>,
    join: JoinHandle<()>,
    events: Arc<EventBus>,
}

impl<T> TaskHandle<T> {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn graph_generation(&self) -> &GraphGenerationId {
        &self.graph_generation
    }

    pub fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            self.events.publish(KernelEvent::TaskCancelled(self.id));
        }
    }

    pub fn join(self) -> thread::Result<T> {
        self.join.join()?;
        Ok(self
            .receiver
            .recv()
            .expect("task worker exited without sending a result"))
    }
}

#[derive(Clone, Copy)]
pub struct TaskScope<'a> {
    runtime: &'a TaskRuntime,
    graph_generation: &'a GraphGenerationId,
    authority: &'a Authority,
    plugin: Option<&'a PluginId>,
}

impl<'a> TaskScope<'a> {
    #[cfg(test)]
    pub(crate) fn new(
        runtime: &'a TaskRuntime,
        graph_generation: &'a GraphGenerationId,
        authority: &'a Authority,
    ) -> Self {
        Self {
            runtime,
            graph_generation,
            authority,
            plugin: None,
        }
    }

    pub(crate) fn new_owned(
        runtime: &'a TaskRuntime,
        graph_generation: &'a GraphGenerationId,
        authority: &'a Authority,
        plugin: &'a PluginId,
    ) -> Self {
        Self {
            runtime,
            graph_generation,
            authority,
            plugin: Some(plugin),
        }
    }

    pub fn graph_generation(&self) -> &GraphGenerationId {
        self.graph_generation
    }

    pub fn authority(&self) -> &Authority {
        self.authority
    }

    pub fn plugin(&self) -> Option<&PluginId> {
        self.plugin
    }

    pub fn spawn<T, F>(&self, requested_authority: &Authority, worker: F) -> TaskHandle<T>
    where
        T: Send + 'static,
        F: FnOnce(CancellationToken) -> T + Send + 'static,
    {
        self.runtime.spawn_scoped(
            self.plugin,
            self.graph_generation,
            self.authority,
            requested_authority,
            worker,
        )
    }
}

#[derive(Debug)]
struct OwnedTask {
    id: u64,
    graph_generation: GraphGenerationId,
    cancelled: Weak<AtomicBool>,
}

#[derive(Debug)]
struct OwnedCall {
    id: u64,
    graph_generation: Option<GraphGenerationId>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct TaskRuntime {
    next_id: AtomicU64,
    next_call_id: AtomicU64,
    events: Arc<EventBus>,
    owned: Mutex<BTreeMap<PluginId, Vec<OwnedTask>>>,
    calls: Mutex<BTreeMap<PluginId, Vec<OwnedCall>>>,
}

impl Default for TaskRuntime {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(0),
            next_call_id: AtomicU64::new(0),
            events: Arc::new(EventBus::default()),
            owned: Mutex::new(BTreeMap::new()),
            calls: Mutex::new(BTreeMap::new()),
        }
    }
}

impl TaskRuntime {
    pub fn events(&self) -> Arc<EventBus> {
        Arc::clone(&self.events)
    }

    pub(crate) fn begin_call<'a>(
        &'a self,
        plugin: &PluginId,
        graph_generation: Option<&GraphGenerationId>,
    ) -> LiveCallScope<'a> {
        let id = self.next_call_id.fetch_add(1, Ordering::Relaxed) + 1;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.calls
            .lock()
            .expect("live call mutex poisoned")
            .entry(plugin.clone())
            .or_default()
            .push(OwnedCall {
                id,
                graph_generation: graph_generation.cloned(),
                cancelled: Arc::clone(&cancelled),
            });
        LiveCallScope {
            runtime: self,
            plugin: plugin.clone(),
            id,
            cancellation: CallCancellationToken { cancelled },
        }
    }

    fn finish_call(&self, plugin: &PluginId, id: u64) {
        let mut calls = self.calls.lock().expect("live call mutex poisoned");
        let Some(plugin_calls) = calls.get_mut(plugin) else {
            return;
        };
        plugin_calls.retain(|call| call.id != id);
        if plugin_calls.is_empty() {
            calls.remove(plugin);
        }
    }

    pub(crate) fn cancel_calls(
        &self,
        plugin: &PluginId,
        graph_generation: Option<&GraphGenerationId>,
    ) -> usize {
        self.calls
            .lock()
            .expect("live call mutex poisoned")
            .get(plugin)
            .into_iter()
            .flatten()
            .filter(|call| call.graph_generation.as_ref() == graph_generation)
            .filter(|call| !call.cancelled.swap(true, Ordering::AcqRel))
            .count()
    }

    #[cfg(test)]
    pub(crate) fn active_call_count(&self, plugin: &PluginId) -> usize {
        self.calls
            .lock()
            .expect("live call mutex poisoned")
            .get(plugin)
            .map_or(0, Vec::len)
    }

    pub fn spawn<T, F>(
        &self,
        graph_generation: &GraphGenerationId,
        parent_authority: &Authority,
        requested_authority: &Authority,
        worker: F,
    ) -> TaskHandle<T>
    where
        T: Send + 'static,
        F: FnOnce(CancellationToken) -> T + Send + 'static,
    {
        self.spawn_scoped(
            None,
            graph_generation,
            parent_authority,
            requested_authority,
            worker,
        )
    }

    fn spawn_scoped<T, F>(
        &self,
        owner: Option<&PluginId>,
        graph_generation: &GraphGenerationId,
        parent_authority: &Authority,
        requested_authority: &Authority,
        worker: F,
    ) -> TaskHandle<T>
    where
        T: Send + 'static,
        F: FnOnce(CancellationToken) -> T + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let cancelled = Arc::new(AtomicBool::new(false));
        if let Some(owner) = owner {
            let mut owned = self.owned.lock().expect("task ownership mutex poisoned");
            let tasks = owned.entry(owner.clone()).or_default();
            tasks.retain(|task| task.cancelled.strong_count() != 0);
            tasks.push(OwnedTask {
                id,
                graph_generation: graph_generation.clone(),
                cancelled: Arc::downgrade(&cancelled),
            });
        }
        let token = CancellationToken {
            cancelled: Arc::clone(&cancelled),
            authority: parent_authority.attenuate(requested_authority),
            graph_generation: graph_generation.clone(),
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let join = thread::spawn(move || {
            let output = worker(token);
            let _ = sender.send(output);
        });

        TaskHandle {
            id,
            graph_generation: graph_generation.clone(),
            cancelled,
            receiver,
            join,
            events: Arc::clone(&self.events),
        }
    }

    pub fn cancel_plugin(&self, plugin: &PluginId) -> usize {
        let tasks = self
            .owned
            .lock()
            .expect("task ownership mutex poisoned")
            .remove(plugin)
            .unwrap_or_default();
        self.cancel_tasks(tasks)
    }

    pub(crate) fn cancel_plugin_generation(
        &self,
        plugin: &PluginId,
        graph_generation: Option<&GraphGenerationId>,
    ) -> usize {
        let Some(graph_generation) = graph_generation else {
            return 0;
        };
        let tasks = {
            let mut owned = self.owned.lock().expect("task ownership mutex poisoned");
            let Some(plugin_tasks) = owned.get_mut(plugin) else {
                return 0;
            };
            let (matching, retained): (Vec<_>, Vec<_>) = std::mem::take(plugin_tasks)
                .into_iter()
                .filter(|task| task.cancelled.strong_count() != 0)
                .partition(|task| &task.graph_generation == graph_generation);
            *plugin_tasks = retained;
            let remove_owner = plugin_tasks.is_empty();
            if remove_owner {
                owned.remove(plugin);
            }
            matching
        };
        self.cancel_tasks(tasks)
    }

    fn cancel_tasks(&self, tasks: Vec<OwnedTask>) -> usize {
        tasks
            .into_iter()
            .filter_map(|task| {
                task.cancelled
                    .upgrade()
                    .map(|cancelled| (task.id, cancelled))
            })
            .filter(|(id, cancelled)| {
                if cancelled.swap(true, Ordering::AcqRel) {
                    return false;
                }
                self.events.publish(KernelEvent::TaskCancelled(*id));
                true
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CapabilityId, ComponentManifest, ConfigContribution, PluginManifest, ResolvedHarness,
    };
    use std::thread;

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn generation_with_authority(authority: &Authority) -> GraphGenerationId {
        ResolvedHarness::resolve(
            Vec::<PluginManifest>::new(),
            Vec::<ComponentManifest>::new(),
            Vec::<ConfigContribution>::new(),
            authority,
        )
        .unwrap()
        .generation()
        .clone()
    }

    fn generation() -> GraphGenerationId {
        generation_with_authority(&Authority::default())
    }

    #[test]
    fn live_call_scope_closes_on_drop() {
        let runtime = TaskRuntime::default();
        let owner = PluginId::parse("fixture.call-owner").unwrap();
        {
            let call = runtime.begin_call(&owner, None);
            assert_ne!(call.id(), 0);
            assert!(!call.cancellation_token().is_cancelled());
            assert_eq!(runtime.active_call_count(&owner), 1);
        }
        assert_eq!(runtime.active_call_count(&owner), 0);
    }

    #[test]
    fn live_call_cancellation_is_owner_scoped() {
        let runtime = TaskRuntime::default();
        let owner = PluginId::parse("fixture.call-owner").unwrap();
        let other = PluginId::parse("fixture.other-owner").unwrap();
        let owned = runtime.begin_call(&owner, None);
        let unrelated = runtime.begin_call(&other, None);

        assert_eq!(runtime.cancel_calls(&owner, None), 1);
        assert!(owned.cancellation_token().is_cancelled());
        assert!(!unrelated.cancellation_token().is_cancelled());
        assert_eq!(runtime.cancel_calls(&owner, None), 0);
    }

    #[test]
    fn live_call_cancellation_is_generation_scoped() {
        let runtime = TaskRuntime::default();
        let owner = PluginId::parse("fixture.call-owner").unwrap();
        let first_generation = generation();
        let second_generation =
            generation_with_authority(&Authority::new([capability("generation.second")]));
        let first = runtime.begin_call(&owner, Some(&first_generation));
        let second = runtime.begin_call(&owner, Some(&second_generation));

        assert_eq!(runtime.cancel_calls(&owner, Some(&first_generation)), 1);
        assert!(first.cancellation_token().is_cancelled());
        assert!(!second.cancellation_token().is_cancelled());
    }

    #[test]
    fn blocking_cancellation_is_observable() {
        let runtime = TaskRuntime::default();
        let events = runtime.events();
        let event_receiver = events.subscribe();
        let authority = Authority::default();

        let handle = runtime.spawn(&generation(), &authority, &authority, |token| {
            while !token.is_cancelled() {
                thread::yield_now();
            }
            42
        });
        let id = handle.id();
        handle.cancel();

        assert_eq!(
            event_receiver.recv().unwrap(),
            KernelEvent::TaskCancelled(id)
        );
        assert_eq!(handle.join().unwrap(), 42);
    }

    #[test]
    fn blocking_task_authority_is_attenuated_from_parent() {
        let runtime = TaskRuntime::default();
        let read = capability("fs.read");
        let write = capability("fs.write");
        let parent = Authority::new([read.clone()]);
        let requested = Authority::new([read.clone(), write.clone()]);

        let handle = runtime.spawn(&generation(), &parent, &requested, move |token| {
            (
                token.authority().permits(&read),
                token.authority().permits(&write),
            )
        });

        assert_eq!(handle.join().unwrap(), (true, false));
    }

    #[test]
    fn task_scope_pins_generation_and_parent_authority() {
        let runtime = TaskRuntime::default();
        let read = capability("fs.read");
        let write = capability("fs.write");
        let authority = Authority::new([read.clone()]);
        let generation = generation();
        let scope = TaskScope::new(&runtime, &generation, &authority);

        assert_eq!(scope.graph_generation(), &generation);
        assert_eq!(scope.authority(), &authority);
        assert_eq!(scope.plugin(), None);
        let requested = Authority::new([read.clone(), write.clone()]);
        let handle = scope.spawn(&requested, move |token| {
            (
                token.authority().permits(&read),
                token.authority().permits(&write),
            )
        });
        assert_eq!(handle.join().unwrap(), (true, false));
    }

    #[test]
    fn plugin_task_cancellation_is_scoped_to_the_owner() {
        let runtime = TaskRuntime::default();
        let authority = Authority::default();
        let generation = generation();
        let owner = PluginId::parse("fixture.controller-owner").unwrap();
        let other = PluginId::parse("fixture.other-owner").unwrap();
        let owner_scope = TaskScope::new_owned(&runtime, &generation, &authority, &owner);
        let other_scope = TaskScope::new_owned(&runtime, &generation, &authority, &other);

        let owned = owner_scope.spawn(&authority, |token| {
            while !token.is_cancelled() {
                thread::yield_now();
            }
            token.graph_generation().clone()
        });
        let unrelated = other_scope.spawn(&authority, |token| token.is_cancelled());

        assert_eq!(owner_scope.plugin(), Some(&owner));
        assert_eq!(runtime.cancel_plugin(&owner), 1);
        assert_eq!(owned.join().unwrap(), generation);
        assert!(!unrelated.join().unwrap());
        assert_eq!(runtime.cancel_plugin(&owner), 0);
    }

    #[test]
    fn plugin_task_cancellation_is_generation_scoped() {
        let runtime = TaskRuntime::default();
        let authority = Authority::default();
        let first_generation = generation();
        let second_generation =
            generation_with_authority(&Authority::new([capability("generation.second")]));
        let owner = PluginId::parse("fixture.controller-owner").unwrap();
        let first_scope = TaskScope::new_owned(&runtime, &first_generation, &authority, &owner);
        let second_scope = TaskScope::new_owned(&runtime, &second_generation, &authority, &owner);
        let release_second = Arc::new(AtomicBool::new(false));
        let release_second_worker = Arc::clone(&release_second);

        let first = first_scope.spawn(&authority, |token| {
            while !token.is_cancelled() {
                thread::yield_now();
            }
            token.graph_generation().clone()
        });
        let second = second_scope.spawn(&authority, move |token| {
            while !release_second_worker.load(Ordering::Acquire) {
                thread::yield_now();
            }
            token.is_cancelled()
        });

        assert_eq!(
            runtime.cancel_plugin_generation(&owner, Some(&first_generation)),
            1
        );
        release_second.store(true, Ordering::Release);
        assert_eq!(first.join().unwrap(), first_generation);
        assert!(!second.join().unwrap());
    }

    #[test]
    fn task_and_worker_remain_pinned_to_starting_graph_generation() {
        let runtime = TaskRuntime::default();
        let authority = Authority::default();
        let starting_generation = generation();
        let expected_worker_generation = starting_generation.clone();

        let handle = runtime.spawn(&starting_generation, &authority, &authority, move |token| {
            token.graph_generation().clone()
        });
        let replacement_generation =
            generation_with_authority(&Authority::new([capability("generation.replacement")]));

        assert_ne!(replacement_generation, starting_generation);
        assert_ne!(handle.graph_generation(), &replacement_generation);
        assert_eq!(handle.graph_generation(), &starting_generation);
        assert_eq!(handle.join().unwrap(), expected_worker_generation);
    }
}
