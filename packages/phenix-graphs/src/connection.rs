//! Bounded FIFO transport and retained per-emission atomic fanout.
//!
//! These types own payloads, not readiness history. Strict readiness policies
//! inspect only queue heads. The explicit CorrelatedAll policy may extract an
//! authenticated request behind another request while preserving per-request
//! order and the same fixed physical queue capacity; no extra buffer is owned.

pub use crate::value::Leased;
use crate::{GraphOrigin, TypeIdentity, Value};
use crate::runtime::RequestId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

/// An immutable payload bound to its capture metadata.
///
/// ```compile_fail,E0616
/// use phenix_graph::{connection::Packet, Value};
/// let mut packet = Packet::new(Value::local(1_i64));
/// packet.value = Value::local(2_i64);
/// ```
#[derive(Clone)]
pub struct Packet {
    value: Value,
    pub origin: Option<GraphOrigin>,
    pub(crate) metadata: Option<Arc<crate::runtime::CaptureMetadata>>,
}
impl Packet {
    pub fn new(value: Value) -> Self {
        Self {
            value,
            origin: None,
            metadata: None,
        }
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
    /// Consume the envelope, retaining all payload code/resource leases.
    pub fn into_value(self) -> Value {
        self.value
    }
    pub fn with_lease(mut self, lease: Arc<dyn Send + Sync>) -> Self {
        self.value = self.value.with_lease(lease);
        self
    }
    pub fn with_origin(mut self, origin: GraphOrigin) -> Self {
        self.origin = Some(origin);
        self
    }
    pub fn metadata(&self) -> Option<&crate::runtime::CaptureMetadata> {
        self.metadata.as_deref()
    }
}
impl fmt::Debug for Packet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Packet")
            .field("value", &self.value)
            .field("origin", &self.origin)
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

/// One producer-to-reader FIFO. Closing the writer preserves accepted packets.
/// Closing the reader returns accepted undelivered packets for explicit disposal.
pub struct ConnectionQueue {
    capacity: NonZeroUsize,
    identity: TypeIdentity,
    packets: VecDeque<Packet>,
    // Bounded by the occupied queue: only request-owned committed packets
    // contribute, and the last removal drops its key.
    request_counts: BTreeMap<RequestId, usize>,
    writer_closed: bool,
    reader_closed: bool,
}
impl ConnectionQueue {
    pub fn new(capacity: NonZeroUsize, identity: TypeIdentity) -> Self {
        Self {
            capacity,
            identity,
            packets: VecDeque::new(),
            request_counts: BTreeMap::new(),
            writer_closed: false,
            reader_closed: false,
        }
    }
    pub fn capacity(&self) -> usize {
        self.capacity.get()
    }
    pub fn len(&self) -> usize {
        self.packets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
    pub fn head(&self) -> Option<&Packet> {
        self.packets.front()
    }
    pub fn packets(&self) -> impl Iterator<Item = &Packet> {
        self.packets.iter()
    }
    /// Passive request membership without inspecting payloads or FIFO offsets.
    /// The queue is the sole owner of this committed-packet count.
    pub(crate) fn request_count(&self, request: RequestId) -> usize {
        self.request_counts.get(&request).copied().unwrap_or(0)
    }
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }
    pub fn writer_closed(&self) -> bool {
        self.writer_closed
    }
    pub fn reader_closed(&self) -> bool {
        self.reader_closed
    }
    pub fn is_eos(&self) -> bool {
        self.writer_closed && self.is_empty()
    }
    /// Failed admission always returns ownership, including schema errors.
    pub fn push(&mut self, packet: Packet) -> Result<(), (ConnectionError, Packet)> {
        if let Err(error) = self.check(&packet) {
            return Err((error, packet));
        }
        self.enqueue(packet);
        Ok(())
    }
    // Only use after check() has accepted the packet. In particular, atomic
    // fanout runs all destination checks before the first queue mutation.
    fn enqueue(&mut self, packet: Packet) {
        if let Some(request) = packet.metadata().and_then(|metadata| metadata.request()) {
            *self.request_counts.entry(request).or_default() += 1;
        }
        self.packets.push_back(packet);
    }
    fn note_removal(&mut self, packet: &Packet) {
        if let Some(request) = packet.metadata().and_then(|metadata| metadata.request()) {
            let count = self
                .request_counts
                .get_mut(&request)
                .expect("committed request packet must be indexed");
            *count -= 1;
            if *count == 0 {
                self.request_counts.remove(&request);
            }
        }
    }
    pub fn pop(&mut self) -> Option<Packet> {
        let packet = self.packets.pop_front()?;
        self.note_removal(&packet);
        Some(packet)
    }
    /// Explicit correlated-mode extraction, preserving order among all
    /// remaining packets and never increasing the queue's bounded capacity.
    pub fn pop_at(&mut self, offset: usize) -> Option<Packet> {
        let packet = self.packets.remove(offset)?;
        self.note_removal(&packet);
        Some(packet)
    }
    /// Dispose one request's accepted packets without touching any other
    /// request's FIFO positions. Unindexed requests cannot own a packet here.
    fn discard_request(&mut self, request: RequestId) -> usize {
        let Some(count) = self.request_counts.remove(&request) else {
            return 0;
        };
        let before = self.packets.len();
        self.packets.retain(|packet| {
            packet.metadata().and_then(|metadata| metadata.request()) != Some(request)
        });
        debug_assert_eq!(before - self.packets.len(), count);
        count
    }
    pub fn close_writer(&mut self) {
        self.writer_closed = true;
    }
    pub fn close_reader(&mut self) -> VecDeque<Packet> {
        self.reader_closed = true;
        self.request_counts.clear();
        std::mem::take(&mut self.packets)
    }
    fn check(&self, packet: &Packet) -> Result<(), ConnectionError> {
        if self.reader_closed {
            Err(ConnectionError::ReaderClosed)
        } else if self.writer_closed {
            Err(ConnectionError::WriterClosed)
        } else if packet.value.identity() != self.identity {
            Err(ConnectionError::WrongType)
        } else if self.is_full() {
            Err(ConnectionError::Full)
        } else {
            Ok(())
        }
    }
}
impl fmt::Debug for ConnectionQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionQueue")
            .field("capacity", &self.capacity)
            .field("identity", &self.identity)
            .field("occupancy", &self.len())
            .field("writer_closed", &self.writer_closed)
            .field("reader_closed", &self.reader_closed)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionError {
    Full,
    ReaderClosed,
    WriterClosed,
    WrongType,
    DuplicateDestination,
    InvalidDestination,
    ForeignTopology,
    AlreadyCommitted,
    Cancelled,
    /// A kernel callback tried to acquire a session mutex already owned on this thread.
    Reentrant,
    /// Session lock is held elsewhere, and a producer callback cannot safely
    /// wait for it while holding the whole-drain/retirement gate. Caller keeps
    /// ownership of the original input value and may retry after contention.
    WouldBlock,
}
impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "connection: {self:?}")
    }
}
impl std::error::Error for ConnectionError {}

/// Pure synchronous value preparation, never external delivery. The transport
/// retains packet provenance; an adapter cannot relabel the accepted producer.
pub trait EdgeAdapter: Send + Sync {
    fn prepare(&self, value: &Value) -> Result<Value, String>;
}
impl<F> EdgeAdapter for F
where
    F: Fn(&Value) -> Result<Value, String> + Send + Sync,
{
    fn prepare(&self, value: &Value) -> Result<Value, String> {
        self(value)
    }
}
pub struct IdentityEdgeAdapter;
impl EdgeAdapter for IdentityEdgeAdapter {
    fn prepare(&self, value: &Value) -> Result<Value, String> {
        Ok(value.clone())
    }
}

pub struct FanoutTarget {
    pub connection: usize,
    pub preparation: Option<Arc<dyn EdgeAdapter>>,
}
impl FanoutTarget {
    pub fn identity(connection: usize) -> Self {
        Self {
            connection,
            preparation: None,
        }
    }
}

/// A bounded topology allocation. A retained transaction is bound to the exact
/// allocation, not just numeric queue indexes from a different session.
pub struct ConnectionSet {
    identity: Arc<()>,
    queues: Vec<ConnectionQueue>,
    cancelled: bool,
}
impl ConnectionSet {
    pub fn new(queues: Vec<ConnectionQueue>) -> Self {
        Self {
            identity: Arc::new(()),
            queues,
            cancelled: false,
        }
    }
    pub fn queues(&self) -> &[ConnectionQueue] {
        &self.queues
    }
    pub fn pop(&mut self, index: usize) -> Option<Packet> {
        self.queues.get_mut(index)?.pop()
    }
    pub fn pop_at(&mut self, index: usize, offset: usize) -> Option<Packet> {
        self.queues.get_mut(index)?.pop_at(offset)
    }
    /// Explicit request disposal, not delivery or rollback. Unrelated packets
    /// retain their exact FIFO order and queue reader/writer state is unchanged.
    pub fn discard_request(&mut self, request: crate::runtime::RequestId) -> usize {
        self.queues
            .iter_mut()
            .map(|queue| queue.discard_request(request))
            .sum()
    }
    pub fn close_writer(&mut self, index: usize) -> Result<(), ConnectionError> {
        self.queues
            .get_mut(index)
            .ok_or(ConnectionError::InvalidDestination)?
            .close_writer();
        Ok(())
    }
    pub fn close_reader(&mut self, index: usize) -> Result<VecDeque<Packet>, ConnectionError> {
        Ok(self
            .queues
            .get_mut(index)
            .ok_or(ConnectionError::InvalidDestination)?
            .close_reader())
    }
    /// Preparation happens once, before any destination mutates. The resulting
    /// transaction is retained on Full and retried without calling adapters.
    /// The selected topology bounds this allocation by its connection count.
    pub fn prepare(
        &self,
        packet: &Packet,
        targets: &[FanoutTarget],
    ) -> Result<PreparedFanout, PrepareError> {
        if self.cancelled {
            return Err(PrepareError::Connection(ConnectionError::Cancelled));
        }
        let mut seen = BTreeSet::new();
        // Validate the complete selection before executing any preparation code.
        for target in targets {
            if target.connection >= self.queues.len() {
                return Err(PrepareError::Connection(
                    ConnectionError::InvalidDestination,
                ));
            }
            if !seen.insert(target.connection) {
                return Err(PrepareError::Connection(
                    ConnectionError::DuplicateDestination,
                ));
            }
        }
        for target in targets {
            let queue = &self.queues[target.connection];
            if queue.reader_closed {
                return Err(PrepareError::Connection(ConnectionError::ReaderClosed));
            }
            if queue.writer_closed {
                return Err(PrepareError::Connection(ConnectionError::WriterClosed));
            }
        }
        let mut prepared = Vec::with_capacity(targets.len());
        for target in targets {
            let mut value = match &target.preparation {
                Some(preparation) => {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        preparation.prepare(&packet.value)
                    }));
                    match result {
                        Ok(result) => result.map_err(PrepareError::Adapter)?,
                        Err(_) => {
                            return Err(PrepareError::Adapter(
                                "packet preparation panicked".into(),
                            ));
                        }
                    }
                }
                None => packet.value.clone(),
            };
            // New adapter payloads may still depend on the source's code/resources.
            value.retain_leases_from(&packet.value);
            if value.identity() != self.queues[target.connection].identity {
                return Err(PrepareError::Connection(ConnectionError::WrongType));
            }
            prepared.push((
                target.connection,
                Packet {
                    value,
                    origin: packet.origin.clone(),
                    metadata: packet.metadata.clone(),
                },
            ));
        }
        Ok(PreparedFanout {
            owner: self.identity.clone(),
            prepared,
            committed: false,
        })
    }
    /// Check all destinations before committing any. Closed/type errors take
    /// precedence over Full, so an impossible fanout never waits indefinitely on
    /// an unrelated full queue. Successful empty selection is observable success.
    pub fn commit(&mut self, transaction: &mut PreparedFanout) -> Result<(), ConnectionError> {
        if !Arc::ptr_eq(&self.identity, &transaction.owner) {
            return Err(ConnectionError::ForeignTopology);
        }
        if self.cancelled {
            return Err(ConnectionError::Cancelled);
        }
        if transaction.committed {
            return Err(ConnectionError::AlreadyCommitted);
        }
        let mut full = false;
        for (index, packet) in &transaction.prepared {
            match self.queues[*index].check(packet) {
                Ok(()) => {}
                Err(ConnectionError::Full) => full = true,
                Err(error) => return Err(error),
            }
        }
        if full {
            return Err(ConnectionError::Full);
        }
        for (index, packet) in transaction.prepared.drain(..) {
            self.queues[index].enqueue(packet);
        }
        transaction.committed = true;
        Ok(())
    }
    /// Cancellation disposal is explicit and countable, not successful delivery.
    pub fn cancel(&mut self) -> usize {
        self.cancelled = true;
        self.queues
            .iter_mut()
            .map(|queue| queue.close_reader().len())
            .sum()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PrepareError {
    Connection(ConnectionError),
    Adapter(String),
}
impl fmt::Display for PrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fanout preparation: {self:?}")
    }
}
impl std::error::Error for PrepareError {}

pub struct PreparedFanout {
    owner: Arc<()>,
    prepared: Vec<(usize, Packet)>,
    committed: bool,
}
impl PreparedFanout {
    /// Scheduler-only capture projection stamp, before the atomic commit.
    pub(crate) fn stamp_capture(&mut self, connection: usize, name: &str) {
        let (_, packet) = self
            .prepared
            .iter_mut()
            .find(|(index, _)| *index == connection)
            .expect("prepared capture destination");
        if let Some(metadata) = &packet.metadata {
            packet.metadata = Some(Arc::new(metadata.for_capture(name)));
        }
    }
    pub fn retained_packets(&self) -> usize {
        self.prepared.len()
    }
    pub fn is_committed(&self) -> bool {
        self.committed
    }
}
impl fmt::Debug for PreparedFanout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedFanout")
            .field("retained_packets", &self.retained_packets())
            .field("committed", &self.committed)
            .finish()
    }
}
