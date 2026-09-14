from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text()
    if old not in source:
        raise SystemExit(f"expected source fragment missing in {path}: {old[:120]!r}")
    path.write_text(source.replace(old, new, 1))


# Expose the existing transport-generic current-client callable admission seam.
transport = Path("rust/crates/phenix-acp-stdio/src/transport.rs")
source = transport.read_text()
source = source.replace("self.admit_client_callable(", "self.admit_current_client_callable(")
old = '''    fn admit_client_callable(
        &self,
        callable: &CallableRef,
        schema: Type,
    ) -> Result<(), ApplicationError> {'''
new = '''    /// Admit one callable owned by this ACP connection's current client generation.
    ///
    /// Application semantics stay outside the transport. Callers provide the exact
    /// callable schema required by the application contract.
    pub fn admit_current_client_callable(
        &self,
        callable: &CallableRef,
        schema: Type,
    ) -> Result<(), ApplicationError> {'''
if old not in source:
    raise SystemExit("private client callable admission seam missing")
source = source.replace(old, new, 1)
transport.write_text(source)

# Harness owns semantic interaction-handler slots. Admission is injected so this
# module does not depend on the ACP transport crate.
application = Path("rust/crates/phenix-harness/src/application.rs")
source = application.read_text()
source = source.replace(
'''        Acknowledged, ApplicationError, PageInput, SessionChange, SessionCreateInput, SessionInfo,
        SessionInput as ApplicationSessionInput, SessionList, SessionProjection,
        SessionProjectionState, SessionRenameInput, SessionResumeInput, SessionSnapshot,
        SessionUpdate,
    },
    CloseSession, CreateSession, ListSessions, Operation, RenameSession, ResumeSession,
};''',
'''        Acknowledged, ApplicationError, ElicitationHandlerRef, InteractionHandlers, PageInput,
        PermissionHandlerRef, SessionChange, SessionCreateInput, SessionInfo,
        SessionInput as ApplicationSessionInput, SessionList, SessionProjection,
        SessionProjectionState, SessionRenameInput, SessionResumeInput, SessionSnapshot,
        SessionUpdate, SetInteractionHandlersInput,
    },
    CloseSession, CreateSession, ListSessions, Operation, RenameSession, ResumeSession,
    SetInteractionHandlers,
};''',
    1,
)
source = source.replace(
'''pub struct ApplicationWorker {
    harness: PhenixHarness,
    authority: Authority,
    projection: SessionProjectionStore,
    next_session_ordinal: u64,
}''',
'''pub struct ApplicationWorker {
    harness: PhenixHarness,
    authority: Authority,
    projection: SessionProjectionStore,
    interaction_handlers: InteractionHandlers,
    next_session_ordinal: u64,
}''',
    1,
)
source = source.replace(
'''            authority: default_suite_authority(),
            projection: SessionProjectionStore::new()?,
            next_session_ordinal: 1,
        })''',
'''            authority: default_suite_authority(),
            projection: SessionProjectionStore::new()?,
            interaction_handlers: InteractionHandlers {
                permission: None,
                elicitation: None,
            },
            next_session_ordinal: 1,
        })''',
    1,
)
marker = '''    #[must_use]
    pub fn projection_mut(&mut self) -> &mut SessionProjectionStore {
        &mut self.projection
    }

    pub fn invoke('''
replacement = '''    #[must_use]
    pub fn projection_mut(&mut self) -> &mut SessionProjectionStore {
        &mut self.projection
    }

    #[must_use]
    pub fn interaction_handlers(&self) -> &InteractionHandlers {
        &self.interaction_handlers
    }

    pub fn clear_interaction_handlers(&mut self) {
        self.interaction_handlers = InteractionHandlers {
            permission: None,
            elicitation: None,
        };
    }

    /// Dispatch an application operation with access to the connected client's
    /// generic callable admission seam.
    ///
    /// Only interaction-handler registration needs this seam. All other
    /// operations use the ordinary configured runtime dispatcher.
    pub fn invoke_with_client_callables(
        &mut self,
        operation: &ContractId,
        input: PhenixValue,
        mut admit: impl FnMut(
            &phenix_core::CallableRef,
            phenix_core::Type,
        ) -> Result<(), ApplicationError>,
    ) -> Result<PhenixValue, ApplicationError> {
        if operation.as_str() == SetInteractionHandlers::ID {
            return self
                .set_interaction_handlers(decode(input)?, &mut admit)
                .map(|value| value.to_value());
        }
        self.invoke(operation, input)
    }

    pub fn invoke('''
if marker not in source:
    raise SystemExit("application worker projection_mut marker missing")
source = source.replace(marker, replacement, 1)
marker = '''    fn create_session(
        &mut self,
        request: SessionCreateInput,
    ) -> Result<SessionInfo, ApplicationError> {'''
interaction_impl = '''    fn set_interaction_handlers(
        &mut self,
        request: SetInteractionHandlersInput,
        admit: &mut impl FnMut(
            &phenix_core::CallableRef,
            phenix_core::Type,
        ) -> Result<(), ApplicationError>,
    ) -> Result<Acknowledged, ApplicationError> {
        let handlers = request.handlers;

        // Validate/admit every new ref before changing the semantic slots. A
        // failed replacement therefore leaves the previous handlers active.
        if let Some(permission) = &handlers.permission {
            admit(
                permission.reference(),
                <PermissionHandlerRef as ValueCodec>::phenix_type(),
            )?;
        }
        if let Some(elicitation) = &handlers.elicitation {
            admit(
                elicitation.reference(),
                <ElicitationHandlerRef as ValueCodec>::phenix_type(),
            )?;
        }

        self.interaction_handlers = handlers;
        Ok(Acknowledged {})
    }

'''
if marker not in source:
    raise SystemExit("create_session marker missing")
source = source.replace(marker, interaction_impl + marker, 1)

# Add focused worker tests without requiring the ACP transport concrete type.
test_marker = '''    fn rename(id: &str, sequence: u64, title: &str) -> SessionUpdate {
        SessionUpdate {
            session_id: SessionId::parse(id).unwrap(),
            sequence,
            update: SessionChange::Renamed {
                title: title.to_owned(),
            },
        }
    }

    #[test]
    fn worker_session_crud_updates_durable_truth_and_projection() {'''
tests = '''    fn rename(id: &str, sequence: u64, title: &str) -> SessionUpdate {
        SessionUpdate {
            session_id: SessionId::parse(id).unwrap(),
            sequence,
            update: SessionChange::Renamed {
                title: title.to_owned(),
            },
        }
    }

    fn client_callable(contract: &str, reference: &str) -> phenix_core::CallableRef {
        phenix_core::CallableRef::new(
            ContractId::parse(contract).unwrap(),
            phenix_core::CapabilityOwnerId::Client(
                phenix_core::ClientConnectionId::parse("client-1").unwrap(),
            ),
            phenix_core::CapabilityGenerationId::parse("generation-1").unwrap(),
            phenix_core::ReferenceId::parse(reference).unwrap(),
        )
    }

    #[test]
    fn interaction_registration_admits_exact_schemas_before_replacing_slots() {
        let mut worker = application_worker();
        let permission = PermissionHandlerRef(client_callable(
            "phenix.application.permission@1",
            "permission-handler",
        ));
        let elicitation = ElicitationHandlerRef(client_callable(
            "phenix.application.elicitation@1",
            "elicitation-handler",
        ));
        let mut admitted = Vec::new();

        let result = worker.invoke_with_client_callables(
            &ContractId::parse(SetInteractionHandlers::ID).unwrap(),
            SetInteractionHandlersInput {
                handlers: InteractionHandlers {
                    permission: Some(permission.clone()),
                    elicitation: Some(elicitation.clone()),
                },
            }
            .to_value(),
            |callable, schema| {
                admitted.push((callable.clone(), schema));
                Ok(())
            },
        );

        assert!(result.is_ok());
        assert_eq!(admitted.len(), 2);
        assert_eq!(
            admitted[0].1,
            <PermissionHandlerRef as ValueCodec>::phenix_type()
        );
        assert_eq!(
            admitted[1].1,
            <ElicitationHandlerRef as ValueCodec>::phenix_type()
        );
        assert_eq!(worker.interaction_handlers().permission, Some(permission));
        assert_eq!(worker.interaction_handlers().elicitation, Some(elicitation));
    }

    #[test]
    fn failed_interaction_admission_keeps_previous_handler_slots() {
        let mut worker = application_worker();
        let previous = PermissionHandlerRef(client_callable(
            "phenix.application.permission@1",
            "previous-permission",
        ));
        worker.interaction_handlers = InteractionHandlers {
            permission: Some(previous.clone()),
            elicitation: None,
        };
        let replacement = PermissionHandlerRef(client_callable(
            "phenix.application.permission@1",
            "replacement-permission",
        ));

        let error = worker
            .invoke_with_client_callables(
                &ContractId::parse(SetInteractionHandlers::ID).unwrap(),
                SetInteractionHandlersInput {
                    handlers: InteractionHandlers {
                        permission: Some(replacement),
                        elicitation: None,
                    },
                }
                .to_value(),
                |_callable, _schema| {
                    Err(ApplicationError::SchemaMismatch {
                        message: "fixture rejection".to_owned(),
                    })
                },
            )
            .unwrap_err();

        assert!(matches!(error, ApplicationError::SchemaMismatch { .. }));
        assert_eq!(worker.interaction_handlers().permission, Some(previous));
        assert!(worker.interaction_handlers().elicitation.is_none());
    }

    #[test]
    fn worker_session_crud_updates_durable_truth_and_projection() {'''
if test_marker not in source:
    raise SystemExit("application worker test insertion marker missing")
source = source.replace(test_marker, tests, 1)
application.write_text(source)

# Remove this one-shot patcher and restore the ordinary maintenance workflow.
workflow = Path(".github/workflows/sync-maintenance.yml")
source = workflow.read_text()
source = source.replace("          python3 .phenix/work/pr-504-interaction-handlers.py\n", "")
source = source.replace(
    '          git commit -m "feat(application): register interaction handlers"\n',
    '          git commit -m "chore: apply CI autofixes"\n',
)
workflow.write_text(source)
Path(__file__).unlink()
