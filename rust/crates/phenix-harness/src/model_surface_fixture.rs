//! Deterministic model fixture for inspecting the exact surface presented at inference time.
//!
//! Reports only model-visible request state: structured tools plus materialized prompt sections.
//! It deliberately does not query plugin registries or other out-of-band runtime state.

use phenix_core::{ModelInferenceRequest, ModelInferenceResponse, PhenixSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelSurfaceTool {
    pub id: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelSurfaceSection {
    pub source: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelSurfaceReport {
    pub model: String,
    pub tools: Vec<ModelSurfaceTool>,
    pub skills: Vec<ModelSurfaceSection>,
    pub instructions: Vec<ModelSurfaceSection>,
    pub context: Vec<ModelSurfaceSection>,
    pub request: String,
    pub continuation_turns: usize,
}

#[must_use]
pub fn model_surface_report(request: &ModelInferenceRequest) -> ModelSurfaceReport {
    let mut tools = request
        .tools
        .iter()
        .map(|tool| ModelSurfaceTool {
            id: tool.id.as_str().to_owned(),
            description: tool.description.clone(),
            input_schema: schema_json(&tool.input_schema),
            output_schema: schema_json(&tool.output_schema),
        })
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| left.id.cmp(&right.id));

    let input = String::from_utf8_lossy(request.input.as_ref());
    let sections = parse_sections(&input);
    let mut skills = Vec::new();
    let mut instructions = Vec::new();
    let mut context = Vec::new();
    let mut request_text = String::new();

    if sections.is_empty() {
        request_text = input.into_owned();
    } else {
        for section in sections {
            match section.kind.as_str() {
                "skill" => skills.push(section.into_public()),
                "instruction" | "project-instruction" => instructions.push(section.into_public()),
                "request" => request_text = section.content,
                _ => context.push(section.into_public()),
            }
        }
    }

    ModelSurfaceReport {
        model: request.model.as_str().to_owned(),
        tools,
        skills,
        instructions,
        context,
        request: request_text,
        continuation_turns: request.continuation.len(),
    }
}

pub fn model_surface_response(
    request: &ModelInferenceRequest,
) -> Result<ModelInferenceResponse, serde_json::Error> {
    let output = serde_json::to_vec_pretty(&model_surface_report(request))?;
    Ok(ModelInferenceResponse {
        output: output.into(),
        provider_metadata: BTreeMap::from([(
            "implementation".into(),
            serde_json::json!("model-surface-introspection").into(),
        )]),
        usage: Default::default(),
        tool_calls: Vec::new(),
    })
}

fn schema_json(schema: &PhenixSchema) -> Value {
    serde_json::to_value(schema).unwrap_or_else(|_| Value::String(format!("{schema:?}")))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedSection {
    kind: String,
    source: String,
    content: String,
}

impl ParsedSection {
    fn into_public(self) -> ModelSurfaceSection {
        ModelSurfaceSection {
            source: self.source,
            content: self.content,
        }
    }
}

fn parse_sections(input: &str) -> Vec<ParsedSection> {
    let mut sections = Vec::new();
    let mut current: Option<ParsedSection> = None;

    for line in input.lines() {
        if let Some((kind, source)) = parse_header(line) {
            if let Some(mut section) = current.take() {
                if section.content.ends_with('\n') {
                    section.content.pop();
                }
                sections.push(section);
            }
            current = Some(ParsedSection {
                kind,
                source,
                content: String::new(),
            });
            continue;
        }

        if let Some(section) = current.as_mut() {
            if !section.content.is_empty() {
                section.content.push('\n');
            }
            section.content.push_str(line);
        }
    }
    if let Some(section) = current {
        sections.push(section);
    }
    sections
}

fn parse_header(line: &str) -> Option<(String, String)> {
    let body = line.strip_prefix("--- phenix ")?.strip_suffix("] ---")?;
    let (kind, source) = body.split_once(" [")?;
    Some((kind.to_owned(), source.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Bytes, CallableId, Key, ModelCacheControl, ModelId, ModelToolDescriptor, PhenixSchema,
    };

    #[test]
    fn report_reflects_exact_model_visible_tools_and_skill_sections() {
        let request = ModelInferenceRequest {
            model: ModelId::parse("fixture-introspection").unwrap(),
            input: Bytes::new(
                b"--- phenix instruction [phenix] ---\nidentity\n\n--- phenix skill [skills/review/SKILL.md] ---\nreview carefully\n\n--- phenix project-context [README.md] ---\nproject body\n\n--- phenix request [user] ---\nlist what you can see\n"
                    .to_vec(),
            ),
            options: BTreeMap::new(),
            cache: ModelCacheControl::default(),
            tools: vec![ModelToolDescriptor {
                id: CallableId::parse("bash").unwrap(),
                description: "Run a shell command".into(),
                input_schema: PhenixSchema::Table(BTreeMap::from([(
                    Key::parse("command").unwrap(),
                    PhenixSchema::String,
                )])),
                output_schema: PhenixSchema::Any,
            }],
            continuation: Vec::new(),
        };

        let report = model_surface_report(&request);
        assert_eq!(report.tools.len(), 1);
        assert_eq!(report.tools[0].id, "bash");
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].source, "skills/review/SKILL.md");
        assert_eq!(report.skills[0].content, "review carefully");
        assert_eq!(report.instructions[0].source, "phenix");
        assert_eq!(report.context[0].source, "README.md");
        assert_eq!(report.request, "list what you can see");
    }

    #[test]
    fn raw_unmaterialized_input_is_reported_as_the_request() {
        let request = ModelInferenceRequest {
            model: ModelId::parse("fixture-introspection").unwrap(),
            input: Bytes::new(b"hello".to_vec()),
            options: BTreeMap::new(),
            cache: ModelCacheControl::default(),
            tools: Vec::new(),
            continuation: Vec::new(),
        };
        assert_eq!(model_surface_report(&request).request, "hello");
    }
}
