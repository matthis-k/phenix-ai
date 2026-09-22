use phenix_application_interface::types::ApplicationError;
use phenix_core::{
    Authority, CallableId, Kernel, ModelToolCall, ModelToolDescriptor, PhenixValue, Project, Type,
};
use phenix_plugin_catalog::{workspace_service, WorkspaceCommand, WorkspaceResponse};
use std::{collections::BTreeMap, sync::Arc};

type RuntimeToolHandler = Arc<
    dyn Fn(&mut Kernel, &Authority, &ModelToolCall) -> Result<PhenixValue, ApplicationError>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct RuntimeModelTool {
    descriptor: ModelToolDescriptor,
    handler: RuntimeToolHandler,
}

impl RuntimeModelTool {
    #[must_use]
    pub fn new(
        descriptor: ModelToolDescriptor,
        handler: impl Fn(&mut Kernel, &Authority, &ModelToolCall) -> Result<PhenixValue, ApplicationError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            descriptor,
            handler: Arc::new(handler),
        }
    }

    #[must_use]
    pub fn descriptor(&self) -> &ModelToolDescriptor {
        &self.descriptor
    }

    pub fn invoke(
        &self,
        kernel: &mut Kernel,
        authority: &Authority,
        call: &ModelToolCall,
    ) -> Result<PhenixValue, ApplicationError> {
        self.descriptor
            .input_schema
            .parse(&call.input)
            .map_err(|error| ApplicationError::SchemaMismatch {
                message: error.to_string(),
            })?;
        let output = (self.handler)(kernel, authority, call)?;
        self.descriptor
            .output_schema
            .parse(&output)
            .map_err(|error| ApplicationError::InvalidResponse {
                message: format!(
                    "runtime tool {} returned an invalid output: {error}",
                    self.descriptor.id
                ),
            })?;
        Ok(output)
    }
}

#[must_use]
pub fn bash_tool() -> RuntimeModelTool {
    RuntimeModelTool::new(
        ModelToolDescriptor {
            id: CallableId::parse("bash").expect("static bash callable id is valid"),
            description: "Run a Bash command in the selected workspace execution environment. The workspace provider decides whether execution is local, SSH, container-backed, or remote. Input must contain a command string."
                .to_owned(),
            input_schema: Type::Map(Box::new(Type::String)),
            output_schema: Type::Map(Box::new(Type::Any)),
        },
        |kernel, authority, call| {
            let PhenixValue::Map(input) = &call.input else {
                return Err(ApplicationError::SchemaMismatch {
                    message: "bash tool input must be an object containing command".to_owned(),
                });
            };
            let command = match input.get("command") {
                Some(PhenixValue::String(command)) if !command.trim().is_empty() => command.clone(),
                _ => {
                    return Err(ApplicationError::SchemaMismatch {
                        message: "bash tool input requires a non-empty string field command"
                            .to_owned(),
                    })
                }
            };
            let input = serde_json::to_vec(&PhenixValue::from(&WorkspaceCommand::Shell { command }))
                .map_err(|error| ApplicationError::InvalidInput {
                    message: error.to_string(),
                })?;
            let output = kernel
                .invoke(&workspace_service(), &input, authority, None)
                .map_err(|error| ApplicationError::Failed {
                    message: error.to_string(),
                })?;
            let output: PhenixValue =
                serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                    message: error.to_string(),
                })?;
            let response = WorkspaceResponse::try_from(Project(&output)).map_err(|error| {
                ApplicationError::InvalidResponse {
                    message: error.to_string(),
                }
            })?;
            let WorkspaceResponse::Process {
                exit_code,
                stdout,
                stderr,
            } = response
            else {
                return Err(ApplicationError::InvalidResponse {
                    message: "workspace shell returned a non-process response".to_owned(),
                });
            };
            Ok(PhenixValue::Map(BTreeMap::from([
                ("exit_code".to_owned(), PhenixValue::I64(i64::from(exit_code))),
                ("stdout".to_owned(), PhenixValue::String(stdout)),
                ("stderr".to_owned(), PhenixValue::String(stderr)),
            ])))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_suite_authority, PhenixHarness};

    #[test]
    fn default_runtime_exposes_and_executes_backend_neutral_bash_tool() {
        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();

        let tools = harness.runtime_model_tools();
        let bash = tools
            .iter()
            .find(|tool| tool.id.as_str() == "bash")
            .expect("default runtime exposes bash");
        assert!(bash.description.contains("selected workspace"));
        assert_eq!(bash.input_schema, Type::Map(Box::new(Type::String)));

        let call = ModelToolCall {
            call_id: "call-bash".to_owned(),
            callable_id: CallableId::parse("bash").unwrap(),
            input: PhenixValue::Map(BTreeMap::from([(
                "command".to_owned(),
                PhenixValue::String("printf PHENIX_RUNTIME_BASH".to_owned()),
            )])),
        };
        let output = harness
            .invoke_runtime_model_tool(&default_suite_authority(), &call)
            .expect("bash is a runtime tool")
            .unwrap();
        let PhenixValue::Map(output) = output else {
            panic!("bash output must be a JSON-compatible map");
        };
        assert_eq!(
            output.get("stdout"),
            Some(&PhenixValue::String("PHENIX_RUNTIME_BASH".to_owned()))
        );
        assert_eq!(output.get("exit_code"), Some(&PhenixValue::I64(0)));
    }
}
