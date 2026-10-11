//! Cells are owned by one admitted runtime. No raw cell API enters the SDK.
use phenix_contract::{
    Code, CompareExchange, Epoch, Failure, GenerationDigest, OwnerId, PropertyDefinition, Revision,
    Value, ValueAccessError as Error, ValueId, ValueLocation, WireProfile,
};
use std::{
    collections::BTreeMap,
    io::Read,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
};

static NEXT_INCARNATION: AtomicU64 = AtomicU64::new(1);
pub(super) fn hash_bytes(bytes: &[u8]) -> Vec<u8> {
    digest_bytes(&phenix_artifact::digest(bytes))
}
pub(super) fn digest_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            let digit = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
            digit(p[0]) * 16 + digit(p[1])
        })
        .collect()
}
fn process_nonce() -> Result<&'static [u8; 32], Failure> {
    static NONCE: OnceLock<Result<[u8; 32], Failure>> = OnceLock::new();
    NONCE
        .get_or_init(|| {
            let mut bytes = [0; 32];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut bytes))
                .map_err(|e| {
                    Failure::new(
                        Code::InfrastructureFailure,
                        format!("owner identity entropy: {e}"),
                    )
                })?;
            Ok(bytes)
        })
        .as_ref()
        .map_err(Clone::clone)
}
#[derive(Clone)]
struct Snapshot {
    revision: Revision,
    epoch: Epoch,
    value: Arc<Value>,
}
impl Snapshot {
    fn value(&self) -> Value {
        Value::List(vec![
            Value::U64(self.revision.0),
            Value::U64(self.epoch.0),
            (*self.value).clone(),
        ])
    }
}
struct Cell {
    location: ValueLocation,
    owner: String,
    property: PropertyDefinition,
    snapshot: Snapshot,
}
struct Subscription {
    root: u64,
    caller: String,
    ticket: Option<u64>,
    location: ValueLocation,
    revision: Revision,
}
pub(super) struct Cells {
    generation: GenerationDigest,
    owners: BTreeMap<String, OwnerId>,
    cells: BTreeMap<ValueId, Cell>,
    subscriptions: BTreeMap<u64, Subscription>,
    next_cursor: u64,
    epoch: Epoch,
}
impl Cells {
    pub(super) fn new(digest: &[u8]) -> Result<Self, Failure> {
        Ok(Self {
            generation: GenerationDigest::from_bytes(digest)?,
            owners: BTreeMap::new(),
            cells: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            next_cursor: 0,
            epoch: Epoch(0),
        })
    }
    pub(super) fn register_owner(&mut self, plugin: &str) -> Result<(), Failure> {
        let mut incarnation = NEXT_INCARNATION.load(Ordering::Relaxed);
        loop {
            let next = incarnation.checked_add(1).ok_or_else(|| {
                Failure::new(Code::InfrastructureFailure, "owner incarnation exhausted")
            })?;
            match NEXT_INCARNATION.compare_exchange_weak(
                incarnation,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => incarnation = current,
            }
        }
        let bytes = Value::List(vec![
            Value::Bytes(process_nonce()?.to_vec()),
            Value::U64(u64::from(std::process::id())),
            Value::Bytes(self.generation.as_bytes().to_vec()),
            Value::String(plugin.into()),
            Value::U64(incarnation),
        ])
        .encode()?;
        self.owners
            .insert(plugin.into(), OwnerId::from_bytes(&hash_bytes(&bytes))?);
        Ok(())
    }
    pub(super) fn create(
        &mut self,
        plugin: &str,
        property: &PropertyDefinition,
        initial: Value,
    ) -> Result<ValueLocation, Failure> {
        if self.cells.len() >= phenix_contract::MAX_COUNT
            || self
                .cells
                .values()
                .any(|c| c.owner == plugin && c.property.id == property.id)
            || !property.schema.accepts(&initial)
        {
            return Err(Failure::new(
                Code::SchemaRejected,
                "duplicate property or invalid initial schema",
            ));
        }
        initial.encode_profile(WireProfile::V2)?;
        let owner = self.owners[plugin];
        let mut bytes = owner.as_bytes().to_vec();
        bytes.extend(property.id.as_bytes());
        let cell = ValueId::from_bytes(&hash_bytes(&bytes)[..16])?;
        if self.cells.contains_key(&cell) {
            return Err(Failure::new(
                Code::InfrastructureFailure,
                "cell identity collision",
            ));
        }
        let location = ValueLocation {
            generation: self.generation,
            owner,
            cell,
            schema: property.schema.id()?,
        };
        self.cells.insert(
            cell,
            Cell {
                location,
                owner: plugin.into(),
                property: property.clone(),
                snapshot: Snapshot {
                    revision: Revision(0),
                    epoch: Epoch(0),
                    value: Arc::new(initial),
                },
            },
        );
        Ok(location)
    }
    fn cell(&self, r: &ValueLocation) -> Result<&Cell, Error> {
        if r.generation != self.generation {
            return Err(Error::StaleReference);
        }
        if !self.owners.values().any(|owner| *owner == r.owner) {
            return Err(Error::OwnerGone);
        }
        let c = self.cells.get(&r.cell).ok_or(Error::StaleReference)?;
        if c.location.owner != r.owner {
            return Err(Error::StaleReference);
        }
        if c.location.schema != r.schema {
            return Err(Error::SchemaMismatch);
        }
        Ok(c)
    }
    pub(super) fn identity(&self, r: &ValueLocation) -> Result<(&str, &PropertyDefinition), Error> {
        let c = self.cell(r)?;
        Ok((&c.owner, &c.property))
    }
    pub(super) fn read(&self, r: &ValueLocation) -> Result<Value, Error> {
        Ok(self.cell(r)?.snapshot.value())
    }
    pub(super) fn snapshot(&self, locations: &[ValueLocation]) -> Result<Value, Error> {
        Ok(Value::List(vec![
            Value::U64(self.epoch.0),
            Value::List(
                locations
                    .iter()
                    .map(|r| self.read(r))
                    .collect::<Result<_, _>>()?,
            ),
        ]))
    }
    pub(super) fn commit(&mut self, edits: &[CompareExchange]) -> Result<Value, Error> {
        let epoch = self.epoch.0.checked_add(1).ok_or(Error::EpochExhausted)?;
        for edit in edits {
            let c = self.cell(&edit.location)?;
            if !c.property.schema.accepts(&edit.value) {
                return Err(Error::SchemaMismatch);
            }
            if edit.expected != c.snapshot.revision {
                return Err(Error::Conflict {
                    expected: edit.expected,
                    current: c.snapshot.revision,
                });
            }
            c.snapshot
                .revision
                .0
                .checked_add(1)
                .ok_or(Error::RevisionExhausted)?;
        }
        let mut snapshots = vec![];
        for edit in edits {
            let c = self.cells.get_mut(&edit.location.cell).unwrap();
            c.snapshot = Snapshot {
                revision: Revision(c.snapshot.revision.0 + 1),
                epoch: Epoch(epoch),
                value: Arc::new(edit.value.clone()),
            };
            snapshots.push(c.snapshot.value());
        }
        self.epoch = Epoch(epoch);
        Ok(Value::List(vec![Value::U64(epoch), Value::List(snapshots)]))
    }
    pub(super) fn subscribe(
        &mut self,
        root: u64,
        caller: &str,
        ticket: Option<u64>,
        r: ValueLocation,
    ) -> Result<Value, Error> {
        if self.subscriptions.len() >= phenix_contract::MAX_COUNT {
            return Err(Error::Denied);
        }
        let snapshot = self.cell(&r)?.snapshot.clone();
        let cursor = self
            .next_cursor
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        self.next_cursor = cursor;
        self.subscriptions.insert(
            cursor,
            Subscription {
                root,
                caller: caller.into(),
                ticket,
                location: r,
                revision: snapshot.revision,
            },
        );
        Ok(Value::List(vec![Value::U64(cursor), snapshot.value()]))
    }
    pub(super) fn cursor(
        &self,
        root: u64,
        caller: &str,
        ticket: Option<u64>,
        id: u64,
    ) -> Result<ValueLocation, Error> {
        let s = self.subscriptions.get(&id).ok_or(Error::StaleReference)?;
        if s.root != root || s.caller != caller || s.ticket != ticket {
            return Err(Error::Denied);
        }
        Ok(s.location)
    }
    pub(super) fn next(
        &mut self,
        root: u64,
        caller: &str,
        ticket: Option<u64>,
        id: u64,
    ) -> Result<Value, Error> {
        let r = self.cursor(root, caller, ticket, id)?;
        let snapshot = self.cell(&r)?.snapshot.clone();
        let subscription = self.subscriptions.get_mut(&id).unwrap();
        if snapshot.revision == subscription.revision {
            return Ok(Value::Null);
        }
        subscription.revision = snapshot.revision;
        Ok(snapshot.value())
    }
    pub(super) fn unsubscribe(
        &mut self,
        root: u64,
        caller: &str,
        ticket: Option<u64>,
        id: u64,
    ) -> Result<Value, Error> {
        self.cursor(root, caller, ticket, id)?;
        self.subscriptions.remove(&id);
        Ok(Value::Null)
    }
    pub(super) fn release_ticket(&mut self, ticket: u64) {
        self.subscriptions.retain(|_, s| s.ticket != Some(ticket));
    }
    pub(super) fn release_root(&mut self, root: u64) {
        self.subscriptions.retain(|_, s| s.root != root);
    }
    pub(super) fn initialized(&self, owner: &str, count: usize) -> bool {
        self.cells.values().filter(|c| c.owner == owner).count() == count
    }
    pub(super) fn clear(&mut self) {
        self.cells.clear();
        self.owners.clear();
        self.subscriptions.clear();
    }
    pub(super) fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"cells":self.cells.len(),"subscriptions":self.subscriptions.len(),"epoch":self.epoch.0,"revisions":self.cells.values().map(|c|serde_json::json!({"owner":c.owner,"property":c.property.id,"revision":c.snapshot.revision.0,"epoch":c.snapshot.epoch.0})).collect::<Vec<_>>()})
    }
}
