use crate::{CallableDescriptor, CallableId, CallableKind, SessionId};
use phenix_core::{CallableRef, CapabilityGenerationId, CapabilityOwnerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};

domain_id_type!(ClientToolAdmissionId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientToolDefinition {
    pub descriptor: CallableDescriptor,
    pub invoke: CallableRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientToolDefinitionError {
    NotATool,
}

impl Display for ClientToolDefinitionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotATool => f.write_str("client tool descriptor must have kind Tool"),
        }
    }
}

impl std::error::Error for ClientToolDefinitionError {}

impl ClientToolDefinition {
    pub fn new(
        descriptor: CallableDescriptor,
        invoke: CallableRef,
    ) -> Result<Self, ClientToolDefinitionError> {
        if descriptor.kind != CallableKind::Tool {
            return Err(ClientToolDefinitionError::NotATool);
        }
        Ok(Self { descriptor, invoke })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClientToolAdmission {
    pub id: ClientToolAdmissionId,
    pub session_id: SessionId,
    pub owner: CapabilityOwnerId,
    pub generation: CapabilityGenerationId,
    pub tool: ClientToolDefinition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientToolAdmissionError {
    DuplicateCallable {
        session_id: SessionId,
        callable: CallableId,
    },
    StaleAdmission(ClientToolAdmissionId),
}

impl Display for ClientToolAdmissionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCallable {
                session_id,
                callable,
            } => {
                write!(f, "session {session_id} already admits callable {callable}")
            }
            Self::StaleAdmission(id) => write!(f, "client tool admission {id} is stale"),
        }
    }
}

impl std::error::Error for ClientToolAdmissionError {}

#[derive(Clone, Debug, Default)]
pub struct ClientToolAdmissions {
    next_id: u64,
    by_id: BTreeMap<ClientToolAdmissionId, ClientToolAdmission>,
    by_session_callable: BTreeMap<(SessionId, CallableId), ClientToolAdmissionId>,
    by_owner_generation:
        BTreeMap<(CapabilityOwnerId, CapabilityGenerationId), BTreeSet<ClientToolAdmissionId>>,
}

impl ClientToolAdmissions {
    pub fn admit(
        &mut self,
        session_id: SessionId,
        tool: ClientToolDefinition,
    ) -> Result<ClientToolAdmission, ClientToolAdmissionError> {
        let key = (session_id.clone(), tool.descriptor.id.clone());
        if self.by_session_callable.contains_key(&key) {
            return Err(ClientToolAdmissionError::DuplicateCallable {
                session_id,
                callable: tool.descriptor.id.clone(),
            });
        }
        self.next_id = self.next_id.saturating_add(1);
        let id = ClientToolAdmissionId::parse(format!("client-tool-{}", self.next_id))
            .expect("generated client tool admission ids are valid");
        let admission = ClientToolAdmission {
            id: id.clone(),
            session_id: session_id.clone(),
            owner: tool.invoke.owner().clone(),
            generation: tool.invoke.generation().clone(),
            tool,
        };
        self.by_session_callable.insert(key, id.clone());
        self.by_owner_generation
            .entry((admission.owner.clone(), admission.generation.clone()))
            .or_default()
            .insert(id.clone());
        self.by_id.insert(id, admission.clone());
        Ok(admission)
    }

    /// Remove the current admission for one session. The application connection
    /// owns authentication; the caller cannot supply or forge an owner id.
    pub fn remove_from_session(
        &mut self,
        session_id: &SessionId,
        id: &ClientToolAdmissionId,
    ) -> Result<ClientToolAdmission, ClientToolAdmissionError> {
        let Some(admission) = self.by_id.get(id) else {
            return Err(ClientToolAdmissionError::StaleAdmission(id.clone()));
        };
        if &admission.session_id != session_id {
            return Err(ClientToolAdmissionError::StaleAdmission(id.clone()));
        }
        self.remove(id, &admission.owner.clone(), &admission.generation.clone())
    }

    fn remove(
        &mut self,
        id: &ClientToolAdmissionId,
        owner: &CapabilityOwnerId,
        generation: &CapabilityGenerationId,
    ) -> Result<ClientToolAdmission, ClientToolAdmissionError> {
        let Some(admission) = self.by_id.get(id) else {
            return Err(ClientToolAdmissionError::StaleAdmission(id.clone()));
        };
        if &admission.owner != owner || &admission.generation != generation {
            return Err(ClientToolAdmissionError::StaleAdmission(id.clone()));
        }
        let admission = self.by_id.remove(id).expect("admission was checked above");
        self.by_session_callable.remove(&(
            admission.session_id.clone(),
            admission.tool.descriptor.id.clone(),
        ));
        let owner_key = (admission.owner.clone(), admission.generation.clone());
        let ids = self
            .by_owner_generation
            .get_mut(&owner_key)
            .expect("admission owner index exists");
        ids.remove(id);
        if ids.is_empty() {
            self.by_owner_generation.remove(&owner_key);
        }
        Ok(admission)
    }

    pub fn retire(&mut self, owner: &CapabilityOwnerId, generation: &CapabilityGenerationId) {
        let Some(ids) = self
            .by_owner_generation
            .get(&(owner.clone(), generation.clone()))
            .cloned()
        else {
            return;
        };
        for id in ids {
            let _ = self.remove(&id, owner, generation);
        }
    }

    /// Finds the currently admitted client tool for one session. Callers use
    /// the returned callable reference only through the generic capability
    /// dispatcher, so removing an admission also removes its execution route.
    #[must_use]
    pub fn admitted(
        &self,
        session_id: &SessionId,
        callable: &CallableId,
    ) -> Option<&ClientToolAdmission> {
        let id = self
            .by_session_callable
            .get(&(session_id.clone(), callable.clone()))?;
        self.by_id.get(id)
    }

    /// Reports whether an admitted tool still owns this callable reference.
    ///
    /// Explicit removal revokes a lifted client callable only after its last
    /// session admission disappears. A client may deliberately admit one
    /// callable into more than one session.
    #[must_use]
    pub fn contains_invoke(&self, invoke: &CallableRef) -> bool {
        self.by_id
            .values()
            .any(|admission| &admission.tool.invoke == invoke)
    }

    #[must_use]
    pub fn descriptors(&self, session_id: &SessionId) -> Vec<CallableDescriptor> {
        let mut descriptors = self
            .by_id
            .values()
            .filter(|admission| &admission.session_id == session_id)
            .map(|admission| admission.tool.descriptor.clone())
            .collect::<Vec<_>>();
        descriptors.sort_by(|left, right| left.id.cmp(&right.id));
        descriptors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CallablePolicy, CapabilitySet};
    use phenix_core::{ContractId, ReferenceId, RuntimeId, Type};

    fn definition(id: &str, generation: &str) -> ClientToolDefinition {
        let descriptor = CallableDescriptor {
            id: CallableId::parse(id).unwrap(),
            kind: CallableKind::Tool,
            description: "fixture client tool".to_owned(),
            input_schema: Type::String,
            output_schema: Type::String,
            capabilities: CapabilitySet::default(),
            policy: CallablePolicy::default(),
        };
        let callable = CallableRef::new(
            ContractId::parse("fixture.callable@1").unwrap(),
            CapabilityOwnerId::Runtime(RuntimeId::parse("fixture.runtime").unwrap()),
            CapabilityGenerationId::parse(generation).unwrap(),
            ReferenceId::parse("fixture.callable").unwrap(),
        );
        ClientToolDefinition::new(descriptor, callable).unwrap()
    }

    #[test]
    fn admission_is_session_scoped_and_stale_handles_cannot_remove_newer_tools() {
        let mut admissions = ClientToolAdmissions::default();
        let session = SessionId::parse("session-a").unwrap();
        let first = admissions
            .admit(session.clone(), definition("fixture.echo", "generation-a"))
            .unwrap();
        assert!(matches!(
            admissions.admit(session.clone(), definition("fixture.echo", "generation-a")),
            Err(ClientToolAdmissionError::DuplicateCallable { .. })
        ));
        admissions.remove_from_session(&session, &first.id).unwrap();
        let second = admissions
            .admit(session.clone(), definition("fixture.echo", "generation-b"))
            .unwrap();
        assert!(matches!(
            admissions.remove_from_session(&session, &first.id),
            Err(ClientToolAdmissionError::StaleAdmission(_))
        ));
        assert_eq!(
            admissions.descriptors(&session),
            vec![second.tool.descriptor]
        );
    }

    #[test]
    fn retirement_only_removes_one_owner_generation() {
        let mut admissions = ClientToolAdmissions::default();
        let session = SessionId::parse("session-a").unwrap();
        let first = admissions
            .admit(session.clone(), definition("fixture.first", "generation-a"))
            .unwrap();
        let second = admissions
            .admit(
                session.clone(),
                definition("fixture.second", "generation-b"),
            )
            .unwrap();
        admissions.retire(&first.owner, &first.generation);
        assert_eq!(
            admissions.descriptors(&session),
            vec![second.tool.descriptor]
        );
    }

    #[test]
    fn admitted_tool_lookup_tracks_explicit_removal() {
        let mut admissions = ClientToolAdmissions::default();
        let session = SessionId::parse("session-a").unwrap();
        let admitted = admissions
            .admit(session.clone(), definition("fixture.echo", "generation-a"))
            .unwrap();
        let callable = admitted.tool.descriptor.id.clone();

        assert_eq!(
            admissions
                .admitted(&session, &callable)
                .map(|tool| &tool.id),
            Some(&admitted.id)
        );
        admissions
            .remove_from_session(&session, &admitted.id)
            .unwrap();
        assert!(admissions.admitted(&session, &callable).is_none());
        assert!(!admissions.contains_invoke(&admitted.tool.invoke));
    }

    #[test]
    fn callable_remains_owned_until_its_last_admission_is_removed() {
        let mut admissions = ClientToolAdmissions::default();
        let first_session = SessionId::parse("session-a").unwrap();
        let second_session = SessionId::parse("session-b").unwrap();
        let first = admissions
            .admit(
                first_session.clone(),
                definition("fixture.first", "generation-a"),
            )
            .unwrap();
        let second = admissions
            .admit(
                second_session.clone(),
                ClientToolDefinition::new(
                    CallableDescriptor {
                        id: CallableId::parse("fixture.second").unwrap(),
                        ..first.tool.descriptor.clone()
                    },
                    first.tool.invoke.clone(),
                )
                .unwrap(),
            )
            .unwrap();

        admissions
            .remove_from_session(&first_session, &first.id)
            .unwrap();
        assert!(admissions.contains_invoke(&first.tool.invoke));
        admissions
            .remove_from_session(&second_session, &second.id)
            .unwrap();
        assert!(!admissions.contains_invoke(&first.tool.invoke));
    }

    #[test]
    fn unrelated_sessions_can_reuse_a_client_callable_id() {
        let mut admissions = ClientToolAdmissions::default();
        let first_session = SessionId::parse("session-a").unwrap();
        let second_session = SessionId::parse("session-b").unwrap();
        let first = admissions
            .admit(
                first_session.clone(),
                definition("fixture.echo", "generation-a"),
            )
            .unwrap();
        let second = admissions
            .admit(
                second_session.clone(),
                definition("fixture.echo", "generation-b"),
            )
            .unwrap();

        assert_eq!(
            admissions.descriptors(&first_session),
            vec![first.tool.descriptor]
        );
        assert_eq!(
            admissions.descriptors(&second_session),
            vec![second.tool.descriptor]
        );
    }

    #[test]
    fn descriptor_projection_is_sorted_by_callable_id() {
        let mut admissions = ClientToolAdmissions::default();
        let session = SessionId::parse("session-a").unwrap();
        admissions
            .admit(session.clone(), definition("fixture.zeta", "generation-a"))
            .unwrap();
        admissions
            .admit(session.clone(), definition("fixture.alpha", "generation-a"))
            .unwrap();

        let ids = admissions
            .descriptors(&session)
            .into_iter()
            .map(|descriptor| descriptor.id.to_string())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["fixture.alpha", "fixture.zeta"]);
    }
}
