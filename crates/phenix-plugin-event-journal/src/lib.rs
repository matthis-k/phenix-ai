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
        if event.sequence != 0 || event.session != request.session || event.run.is_empty() {
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
        return Err(error(
            ErrorCode::InvalidInput,
            "page limit must be positive",
        ));
    }
    let tail = u64::try_from(events.len()).map_err(|_| storage_error())?;
    if query.after > tail {
        return Err(error(
            ErrorCode::InvalidCursor,
            "journal offset is beyond the committed tail",
        ));
    }
    let start = usize::try_from(query.after)
        .map_err(|_| error(ErrorCode::InvalidCursor, "invalid journal cursor offset"))?;
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
        JournalQuery, ObserverCheckpoint, ObserverRequest, RecordKey, Storage,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    fn observer_key(observer: &str) -> CallResult<String, BasicError> {
        if !phenix_sdk::identifier(observer) {
            return Err(CallError::Domain(phenix_basic_contracts::error(
                ErrorCode::InvalidInput,
                "invalid observer identity",
            )));
        }
        Ok(format!("event-observer.v1.{observer}"))
    }

    fn load_observer(
        host: &Host<'_>,
        request: &ObserverRequest,
    ) -> CallResult<(ObserverCheckpoint, u64), BasicError> {
        let key = observer_key(&request.observer)?;
        let (_, events) = load(host, &request.session)?;
        super::validate(&events, &request.session).map_err(CallError::Domain)?;
        let tail = events.last().map_or(0, |event| event.sequence);
        let record = host.invoke::<Storage::read>(
            "Storage",
            &RecordKey {
                session: request.session.clone(),
                key,
            },
        );
        let checkpoint = match record {
            Ok(record) => {
                let saved: ObserverCheckpoint =
                    decode(&record.data).map_err(|_| CallError::Domain(storage_error()))?;
                if saved.session != request.session
                    || saved.observer != request.observer
                    || saved.revision == 0
                    || saved.revision != record.revision
                    || saved.after > tail
                {
                    return Err(CallError::Domain(storage_error()));
                }
                saved
            }
            Err(CallError::Domain(error)) if error.code == ErrorCode::NotFound => {
                ObserverCheckpoint {
                    session: request.session.clone(),
                    observer: request.observer.clone(),
                    after: 0,
                    revision: 0,
                }
            }
            Err(error) => return Err(error),
        };
        Ok((checkpoint, tail))
    }

    #[provide(EventJournal::observer)]
    fn observer(
        _: &mut State,
        host: &Host<'_>,
        request: ObserverRequest,
    ) -> CallResult<ObserverCheckpoint, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        Ok(load_observer(host, &request)?.0)
    }

    #[provide(EventJournal::checkpoint_observer)]
    fn checkpoint_observer(
        _: &mut State,
        host: &Host<'_>,
        request: ObserverCheckpoint,
    ) -> CallResult<ObserverCheckpoint, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let (current, tail) = load_observer(
            host,
            &ObserverRequest {
                session: request.session.clone(),
                observer: request.observer.clone(),
            },
        )?;
        if request.revision != current.revision {
            return Err(CallError::Domain(phenix_basic_contracts::error(
                ErrorCode::Conflict,
                "observer revision changed",
            )));
        }
        if request.after < current.after || request.after > tail {
            return Err(CallError::Domain(phenix_basic_contracts::error(
                ErrorCode::InvalidCursor,
                "observer offset is outside delivered journal bounds",
            )));
        }
        if request.after == current.after {
            return Ok(current);
        }
        let mut saved = request;
        saved.revision = current.revision.checked_add(1).ok_or_else(|| {
            CallError::Domain(phenix_basic_contracts::error(
                ErrorCode::Conflict,
                "observer revision overflow",
            ))
        })?;
        let data = encode(&saved)?;
        let key = observer_key(&saved.observer)?;
        phenix_basic_contracts::transact_one(
            host,
            "Storage",
            &saved.session,
            &key,
            current.revision,
            data,
        )?;
        Ok(saved)
    }

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
            Err(CallError::Domain(error)) if error.code == ErrorCode::NotFound => Ok((0, vec![])),
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
        phenix_basic_contracts::transact_one(
            host,
            "Storage",
            &request.session,
            JOURNAL_KEY,
            revision,
            data,
        )?;
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
        assert_eq!(
            events.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            [1, 2]
        );
        let next = JournalAppend {
            session: "s1".into(),
            events: vec![event("s1", "r2", "three")],
        };
        let events = append_checked(&events, &next).unwrap();
        let a = page_from(
            &events,
            &JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 2,
            },
        )
        .unwrap();
        let b = page_from(
            &events,
            &JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 1,
            },
        )
        .unwrap();
        assert_eq!(a.events.len(), 2);
        assert_eq!(a.next, 2);
        assert_eq!(b.events.len(), 1);
        assert_eq!(b.next, 1);
        assert_eq!(
            page_from(
                &events,
                &JournalQuery {
                    session: "s1".into(),
                    after: a.next,
                    limit: 2,
                }
            )
            .unwrap()
            .events[0]
                .text,
            "three"
        );
    }

    #[test]
    fn reject_future_cursor_but_accept_exact_tail_and_empty_journal() {
        let events = append_checked(
            &[],
            &JournalAppend {
                session: "s1".into(),
                events: vec![event("s1", "r1", "one")],
            },
        )
        .unwrap();
        for after in [2, u64::MAX] {
            assert_eq!(
                page_from(
                    &events,
                    &JournalQuery {
                        session: "s1".into(),
                        after,
                        limit: 5,
                    },
                )
                .unwrap_err()
                .code,
                ErrorCode::InvalidCursor
            );
        }
        let at_tail = page_from(
            &events,
            &JournalQuery {
                session: "s1".into(),
                after: 1,
                limit: 5,
            },
        )
        .unwrap();
        assert!(at_tail.events.is_empty());
        assert_eq!(at_tail.next, 1);
        let empty = page_from(
            &[],
            &JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 5,
            },
        )
        .unwrap();
        assert!(empty.events.is_empty());
        assert_eq!(empty.next, 0);
    }

    #[test]
    fn reject_cross_session_events_and_corrupt_persisted_sequences() {
        let mut item = event("s2", "run", "wrong");
        assert_eq!(
            append_checked(
                &[],
                &JournalAppend {
                    session: "s1".into(),
                    events: vec![item.clone()],
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidInput
        );
        item.session = "s1".into();
        item.sequence = 7;
        assert_eq!(
            append_checked(
                &[],
                &JournalAppend {
                    session: "s1".into(),
                    events: vec![item.clone()],
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            page_from(
                &[item],
                &JournalQuery {
                    session: "s1".into(),
                    after: 0,
                    limit: 1,
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::StorageFailed
        );
    }
}
