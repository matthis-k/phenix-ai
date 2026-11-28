//! Deterministic capability-aware provider used to exercise the real loop.
//! Ordinary Model adapter, native tool schemas, and selected environment
//! catalog; no special scheduler or logger dependency.

use super::{
    Model, ModelBackend, ModelOutput, ModelProfile, ModelStream,
    status::{ProviderStatus, QuotaWindow, Usage},
};
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
    responses: u64,
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
        .capability("usage")
        .capability("quota-status")
        .context_tokens(32_768);
    let backend = FakeBackend {
        catalog,
        environment: environment.into(),
        expected: profile.clone(),
        responses: 0,
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
        self.responses = self.responses.saturating_add(1);
        let report = ModelOutput::Status(fake_status(self.responses));
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
                    report,
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
                    report,
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

fn fake_status(responses: u64) -> ProviderStatus {
    ProviderStatus {
        observed_at: Some(1_800_000_000),
        usage: Some(Usage {
            input_tokens: Some(10),
            output_tokens: Some(3),
            cached_input_tokens: Some(2),
            reasoning_tokens: None,
        }),
        windows: vec![
            QuotaWindow {
                id: "requests-minute".into(),
                scope: "account".into(),
                unit: "requests".into(),
                limit: Some(60),
                remaining: Some(60_u64.saturating_sub(responses)),
                resets_at: Some(1_800_000_060),
            },
            QuotaWindow {
                id: "subscription-week".into(),
                scope: "subscription".into(),
                unit: "credits".into(),
                limit: None,
                remaining: None,
                resets_at: Some(1_800_604_800),
            },
        ],
        retry_at: None,
    }
}
