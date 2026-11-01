//! Frontend-neutral ownership and recovery boundary for a Basic submission.
use super::*;

pub(super) struct SessionAdmission {
    owner: SessionOwnership,
    value: Value,
}

#[derive(Debug)]
pub(super) enum AdmissionError {
    InvalidSubmission(String),
    Ownership(String),
    Recovery(String),
}

impl From<AdmissionError> for String {
    fn from(error: AdmissionError) -> Self {
        match error {
            AdmissionError::InvalidSubmission(message)
            | AdmissionError::Ownership(message)
            | AdmissionError::Recovery(message) => message,
        }
    }
}

pub(super) struct SessionOwnership {
    file: std::fs::File,
}

#[derive(Debug)]
pub(super) enum OwnershipError {
    Busy,
    Storage(String),
}

impl From<String> for OwnershipError {
    fn from(message: String) -> Self {
        Self::Storage(message)
    }
}

impl From<OwnershipError> for String {
    fn from(error: OwnershipError) -> Self {
        match error {
            OwnershipError::Busy => "session already owned by another host".into(),
            OwnershipError::Storage(message) => message,
        }
    }
}

impl SessionOwnership {
    // Observation requires exclusion, but must not recover or execute a run.
    pub(super) fn acquire(session: &str) -> Result<Self, OwnershipError> {
        session_lock(session).map(|file| Self { file })
    }
}

impl SessionAdmission {
    pub(super) fn acquire(
        kernel: &Kernel,
        session: &str,
        submission: Submission,
    ) -> Result<Self, AdmissionError> {
        // Validate before taking ownership or performing recovery writes.
        let value =
            Self::validate(session, submission).map_err(AdmissionError::InvalidSubmission)?;
        let mut owner = SessionOwnership::acquire(session)
            .map_err(|error| AdmissionError::Ownership(error.into()))?;
        recover_owned_run(kernel, session, &mut owner.file).map_err(AdmissionError::Recovery)?;
        Ok(Self { owner, value })
    }

    fn validate(session: &str, submission: Submission) -> Result<Value, String> {
        let Submission::Dispatch { action, payload } = submission else {
            return Err("no active graph execution to cancel".into());
        };
        if action != "phenix.basic.submit" {
            return Err("frontend selected an unknown Basic action".into());
        }
        let value = Value::decode(&payload.0).map_err(|e| format!("{e:?}"))?;
        let prompt = phenix_basic_contracts::PromptSubmitted::from_value(value.clone())
            .map_err(|e| format!("{e:?}"))?;
        if prompt.session != session || prompt.text.is_empty() {
            return Err("invalid Basic submission session or empty prompt".into());
        }
        Ok(value)
    }

    pub(super) fn begin<'a>(
        &self,
        kernel: &'a Kernel,
        scope: ScopedRequest,
    ) -> Result<GraphExecution<'a>, String> {
        kernel
            .begin_action_scoped("phenix.basic.submit", &self.value, scope)
            .map_err(|e| format!("{e:?}"))
    }

    pub(super) fn mark(
        &mut self,
        kernel: &Kernel,
        execution: &GraphExecution<'_>,
        session: &str,
    ) -> Result<bool, String> {
        mark_admitted_run(kernel, execution, session, &mut self.owner.file)
    }

    pub(super) fn complete(&mut self) -> Result<(), String> {
        // Errors and abandonment deliberately retain the crash-recovery marker.
        set_owner_marker(&mut self.owner.file, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission(session: &str, text: &str) -> Submission {
        Submission::Dispatch {
            action: "phenix.basic.submit".into(),
            payload: phenix_sdk::Blob(
                phenix_basic_contracts::PromptSubmitted {
                    session: session.into(),
                    text: text.into(),
                    limits: Default::default(),
                }
                .to_value()
                .encode()
                .unwrap(),
            ),
        }
    }

    #[test]
    fn validates_typed_submission_before_ownership() {
        assert!(SessionAdmission::validate("s1", submission("s1", "hello")).is_ok());
        assert!(SessionAdmission::validate("s1", submission("s2", "hello")).is_err());
        assert!(SessionAdmission::validate("s1", submission("s1", "")).is_err());
        assert!(SessionAdmission::validate("s1", Submission::Cancel).is_err());
        for (action, payload) in [("other", vec![]), ("phenix.basic.submit", vec![255])] {
            assert!(SessionAdmission::validate(
                "s1",
                Submission::Dispatch {
                    action: action.into(),
                    payload: phenix_sdk::Blob(payload),
                }
            )
            .is_err());
        }
    }
}
