//! One domain's behavior, not a graph scheduler or storage mirror.
use phenix_basic_shell::{Prompt, SessionTurn};

#[phenix_sdk::plugin(
    id = "phenix.shell.session",
    version = "0.1.0",
    define(phenix_basic_shell::Session),
    provide(phenix_basic_shell::Session, slot = "Session")
)]
mod plugin {
    use super::*;
    use phenix_basic_shell::Session;

    #[derive(Default)]
    pub struct Sessions {
        next_run: u64,
        active: Option<String>,
    }

    impl Sessions {
        #[provide(Session::admit)]
        pub fn admit(&mut self, prompt: Prompt) -> Result<SessionTurn, String> {
            if prompt.text.trim().is_empty() {
                return Err("empty prompt".into());
            }
            let next = self
                .next_run
                .checked_add(1)
                .ok_or_else(|| "run identity exhausted".to_owned())?;
            let session = self
                .active
                .clone()
                .unwrap_or_else(|| "fixture-session-1".into());
            self.active = Some(session.clone());
            self.next_run = next;
            Ok(SessionTurn {
                session,
                run: format!("fixture-run-{next}"),
                text: prompt.text,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::{ContractMetadata, Portable};

    #[test]
    fn prompt_admits_session_and_monotonic_run() {
        let mut sessions = plugin::Sessions::default();
        let first = sessions
            .admit(Prompt {
                text: "hello".into(),
            })
            .unwrap();
        assert_eq!(first.session, "fixture-session-1");
        assert_eq!(first.run, "fixture-run-1");
        let second = sessions
            .admit(Prompt {
                text: "follow-up".into(),
            })
            .unwrap();
        assert_eq!(second.session, first.session);
        assert_eq!(second.run, "fixture-run-2");
    }

    #[test]
    fn invalid_prompt_preserves_authoritative_state() {
        let mut sessions = plugin::Sessions::default();
        assert!(sessions.admit(Prompt { text: " ".into() }).is_err());
        assert_eq!(
            sessions.admit(Prompt { text: "ok".into() }).unwrap().run,
            "fixture-run-1"
        );
    }

    #[test]
    fn abi_contract_matches_domain_signature_without_exposing_state() {
        let meta = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(meta.provides.len(), 1);
        assert!(meta.requires.is_empty());
        assert!(meta.properties.is_empty());
        let contract = phenix_basic_shell::Session::CONTRACT.owned().unwrap();
        assert_eq!(contract.operations.len(), 1);
        assert_eq!(
            contract.operations[0].request,
            Prompt::SCHEMA.owned().unwrap()
        );
        assert_eq!(
            contract.operations[0].response,
            SessionTurn::SCHEMA.owned().unwrap()
        );
    }
}
