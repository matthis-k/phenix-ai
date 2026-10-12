//! Durable semantic event journal over the selected, revisioned Storage
//! provider. The kernel is unaware of event, session or storage semantics.
use phenix_basic_contracts::{
    error, BasicError, ErrorCode, Event, EventPage, JournalAppend, JournalQuery,
};

const JOURNAL_KEY: &str = "event-journal.v1";

fn storage_error() -> BasicError {
    error(ErrorCode::StorageFailed, "invalid durable journal record")
}

/// Validate persisted data every time it is loaded; a corrupt record is not
/// silently repaired or replayed with forged sequence numbers.
fn validate(events: &[Event], session: &str) -> Result<(), BasicError> {
    for (index, event) in events.iter().enumerate() {
        let sequence = u64::try_from(index)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or_else(storage_error)?;
        if event.sequence != sequence || event.session != session || event.run.is_empty() {
            return Err(storage_error());
        }
    }
    Ok(())
}

/// Assign contiguous sequences only after a valid snapshot has been read.
/// A Storage CAS conflict must be returned to the caller; no duplicate
/// event is assigned or written outside a successful transaction.
pub fn append_checked(
    existing: &[Event],
    request: &JournalAppend,
) -> Result<Vec<Event>, BasicError> {
    validate(existing, &request.session)?;
    let mut updated = existing.to_vec();
    for event in &request.events {
        if event.sequence != 0
            || event.session != request.session
            || event.run.is_empty()
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "invalid submitted journal event",
            ));
        }
        let mut event = event.clone();
        event.sequence = u64::try_from(updated.len())
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| error(ErrorCode::Conflict, "journal sequence overflow"))?;
        updated.push(event);
    }
    Ok(updated)
}

/// Each reader supplies its own exclusive offset. Reading neither mutates
/// durable state nor consumes events for other observers.
pub fn page_from(events: &[Event], query: &JournalQuery) -> Result<EventPage, BasicError> {
    validate(events, &query.session)?;
    if query.limit == 0 {
        return Err(error(ErrorCode::InvalidInput, "page limit must be positive"));
    }
    let start = usize::try_from(query.after).unwrap_or(usize::MAX).min(events.len());
    let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
    let end = start.saturating_add(limit).min(events.len());
    Ok(EventPage {
        events: events[start..end].to_vec(),
        next: events
            .get(end.saturating_sub(1))
            .filter(|_| end > start)
            .map_or(query.after, |event| event.sequence),
    })
}

#[phenix_sdk::plugin(
    id = "phenix.basic.event-journal",
    version = "1.0.0",
    provide(phenix_basic_contracts::EventJournal, slot = "Journal"),
    require(phenix_basic_contracts::Storage, slot = "Storage")
)]
mod plugin {
    use super::{append_checked, page_from, storage_error, JOURNAL_KEY};
    use phenix_basic_contracts::{
        decode, encode, BasicError, ErrorCode, Event, EventJournal, EventPage, JournalAppend,
        JournalQuery, RecordKey, Storage, StorageEdit, StorageTransaction,
    };
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};

    #[derive(Default)]
    pub struct State;

    fn load(host: &Host<'_>, session: &str) -> CallResult<(u64, Vec<Event>), BasicError> {
        let result = host.invoke::<Storage::read>(
            "Storage",
            &RecordKey {
                session: session.into(),
                key: JOURNAL_KEY.into(),
            },
        );
        match result {
            Ok(record) => Ok((
                record.revision,
                decode(&record.data).map_err(|_| CallError::Domain(storage_error()))?,
            )),
            Err(CallError::Domain(error)) if error.code == ErrorCode::NotFound => {
                Ok((0, vec![]))
            }
            Err(error) => Err(error),
        }
    }

    #[provide(EventJournal::append)]
    fn append(
        _: &mut State,
        host: &Host<'_>,
        request: JournalAppend,
    ) -> CallResult<u64, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let (revision, existing) = load(host, &request.session)?;
        let updated = append_checked(&existing, &request).map_err(CallError::Domain)?;
        let count = updated.last().map_or(0, |event| event.sequence);
        if request.events.is_empty() {
            return Ok(count);
        }
        let data = encode(&updated)?;
        let records = host.invoke::<Storage::transact>(
            "Storage",
            &StorageTransaction {
                session: request.session,
                edits: vec![StorageEdit {
                    key: JOURNAL_KEY.into(),
                    expected: revision,
                    data,
                }],
            },
        )?;
        if records.len() != 1 || records[0].revision <= revision {
            return Err(CallError::Kernel(Failure::new(
                Code::PluginFailed,
                "storage transaction returned an invalid revision",
            )));
        }
        Ok(count)
    }

    #[provide(EventJournal::read)]
    fn read(
        _: &mut State,
        host: &Host<'_>,
        request: JournalQuery,
    ) -> CallResult<EventPage, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let (_, events) = load(host, &request.session)?;
        page_from(&events, &request).map_err(CallError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_basic_contracts::EventKind;
    use phenix_sdk::Blob;

    fn event(session: &str, run: &str, text: &str) -> Event {
        Event {
            sequence: 0,
            session: session.into(),
            run: run.into(),
            kind: EventKind::TextDelta,
            call_id: String::new(),
            text: text.into(),
            data: Blob(Vec::new()),
        }
    }

    #[test]
    fn assigned_sequences_and_independent_replay_offsets() {
        let first = JournalAppend {
            session: "s1".into(),
            events: vec![event("s1", "r1", "one"), event("s1", "r1", "two")],
        };
        let events = append_checked(&[], &first).unwrap();
        assert_eq!(events.iter().map(|e| e.sequence).collect::<Vec<_>>(), [1, 2]);
        let next = JournalAppend {
            session: "s1".into(),
            events: vec![event("s1", "r2", "three")],
        };
        let events = append_checked(&events, &next).unwrap();
        let a = page_from(&events, &JournalQuery {
            session: "s1".into(),
            after: 0,
            limit: 2,
        })
        .unwrap();
        let b = page_from(&events, &JournalQuery {
            session: "s1".into(),
            after: 0,
            limit: 1,
        })
        .unwrap();
        assert_eq!(a.events.len(), 2);
        assert_eq!(a.next, 2);
        assert_eq!(b.events.len(), 1);
        assert_eq!(b.next, 1);
        assert_eq!(page_from(&events, &JournalQuery {
            session: "s1".into(),
            after: a.next,
            limit: 2,
        }).unwrap().events[0].text, "three");
    }

    #[test]
    fn reject_cross_session_events_and_corrupt_persisted_sequences() {
        let mut item = event("s2", "run", "wrong");
        assert_eq!(append_checked(&[], &JournalAppend {
            session: "s1".into(),
            events: vec![item.clone()],
        }).unwrap_err().code, ErrorCode::InvalidInput);
        item.session = "s1".into();
        item.sequence = 7;
        assert_eq!(append_checked(&[], &JournalAppend {
            session: "s1".into(),
            events: vec![item.clone()],
        }).unwrap_err().code, ErrorCode::InvalidInput);
        assert_eq!(page_from(&[item], &JournalQuery {
            session: "s1".into(),
            after: 0,
            limit: 1,
        }).unwrap_err().code, ErrorCode::StorageFailed);
    }
}
