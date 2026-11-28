//! Deterministic capability-aware provider used to exercise the real loop.
//! Ordinary Model adapter, native tool schemas, and selected environment
//! catalog; no special scheduler or logger dependency.

use super::{Model, ModelBackend, ModelOutput, ModelProfile, ModelStream};
use crate::{ModelInput, ToolCall, ToolOutcome, ToolValue, harness::ToolCatalog};
use phenix_graph::{ContractValue, Failure};

#[derive(ContractValue)]
#[contract(schema = "agent.fake.arguments")]
pub struct FakeArguments {
    pub text: String,
    pub count: i64,
}

#[derive(Clone)]
struct FakeBackend {
    catalog: ToolCatalog,
    environment: String,
    expected: ModelProfile,
}

/// Select a known tool from the actual catalog. The profile is declared once
/// and supplied by the normal adapter on every response.
pub fn provider(catalog: ToolCatalog, environment: impl Into<String>) -> Model {
    let profile = ModelProfile::new("fake-provider", "fake-capability-model")
        .capability("text")
        .capability("streaming")
        .capability("tools")
        .capability("structured-arguments")
        .capability("reasoning")
        .context_tokens(32_768);
    let backend = FakeBackend {
        catalog,
        environment: environment.into(),
        expected: profile.clone(),
    };
    Model::new("fake-model", profile, move || backend.clone())
}

impl ModelBackend for FakeBackend {
    fn respond(
        &mut self,
        input: ModelInput,
        profile: &ModelProfile,
    ) -> Result<ModelStream, Failure> {
        if profile != &self.expected {
            return Err(Failure::application_message(
                "incorrect runtime model profile",
            ));
        }
        let outputs = match input {
            ModelInput::Prompt(text) => {
                let tool = self
                    .catalog
                    .get(&self.environment)
                    .and_then(|tools| tools.get("repeat"))
                    .ok_or_else(|| {
                        Failure::application_message("selected tool absent from model catalog")
                    })?;
                if tool.arguments != FakeArguments::schema() || tool.result != String::schema() {
                    return Err(Failure::application_message(
                        "incorrect tool catalog schemas",
                    ));
                }
                vec![
                    ModelOutput::Info,
                    ModelOutput::Delta("fake: ".into()),
                    ModelOutput::Tool(ToolCall {
                        name: tool.name.clone(),
                        environment: self.environment.clone(),
                        arguments: ToolValue::new(&FakeArguments { text, count: 2 }),
                    }),
                ]
            }
            ModelInput::ToolResult(result) => match result.outcome() {
                ToolOutcome::Succeeded { output } => vec![
                    ModelOutput::Info,
                    ModelOutput::Delta(output.decode::<String>()?),
                    ModelOutput::Complete,
                ],
                ToolOutcome::Failed { .. } => {
                    return Err(Failure::application_message("fake tool failed"));
                }
            },
        };
        Ok(Box::new(outputs.into_iter().map(Ok)))
    }
}
