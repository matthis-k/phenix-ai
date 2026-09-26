use crate::{
    ContextResourceKind, ExactContextReference, ExecutionContextProjection, ProjectedContextEntry,
};
use phenix_core::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PHENIX_HARNESS_IDENTITY: &str = "You are an AI agent powered by Phenix. The default Phenix application provides a workspace-backed shell tool, durable session history that survives runtime restarts, persistent memory services, and skill/context loading. Active skills appear as instruction sections, and the tools attached to each request are the callable tool surface for that turn. When asked about your environment or memory, report the concrete tools, active skills/context, and supplied conversation history; do not infer that a Phenix subsystem is absent only because it is not exposed as a separate tool.";

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionRole {
    Instruction,
    Context,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionKind {
    HarnessIdentity,
    ProjectInstruction,
    Skill,
    ProjectDocument,
    External,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct PromptSection {
    pub role: PromptSectionRole,
    pub kind: PromptSectionKind,
    pub source: String,
    pub reference: Option<ExactContextReference>,
    pub content: Bytes,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct PromptAssembly {
    pub execution_id: String,
    pub sections: Vec<PromptSection>,
}

/// Assemble the model-facing prompt inputs for one execution.
///
/// The default is intentionally fixed: Harness identity first, project instructions next,
/// skills after project instructions, then non-instruction context. Exact context revisions are
/// included once. Injection sequence breaks ties within each class so output is deterministic.
#[must_use]
pub fn assemble_prompt(projection: &ExecutionContextProjection) -> PromptAssembly {
    let mut entries = projection.entries.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        section_rank(&left.resource.descriptor.kind)
            .cmp(&section_rank(&right.resource.descriptor.kind))
            .then_with(|| left.injection.sequence.cmp(&right.injection.sequence))
            .then_with(|| left.injection.source.cmp(&right.injection.source))
    });

    let mut seen = BTreeSet::new();
    let mut sections = vec![PromptSection {
        role: PromptSectionRole::Instruction,
        kind: PromptSectionKind::HarnessIdentity,
        source: "phenix".into(),
        reference: None,
        content: PHENIX_HARNESS_IDENTITY.as_bytes().to_vec().into(),
    }];

    for entry in entries {
        if !seen.insert(entry.injection.source.clone()) {
            continue;
        }
        sections.push(section(entry));
    }

    PromptAssembly {
        execution_id: projection.execution_id.clone(),
        sections,
    }
}

fn section(entry: &ProjectedContextEntry) -> PromptSection {
    let (role, kind) = match entry.resource.descriptor.kind {
        ContextResourceKind::ProjectInstruction => (
            PromptSectionRole::Instruction,
            PromptSectionKind::ProjectInstruction,
        ),
        ContextResourceKind::Skill => (PromptSectionRole::Instruction, PromptSectionKind::Skill),
        ContextResourceKind::ProjectDocument => (
            PromptSectionRole::Context,
            PromptSectionKind::ProjectDocument,
        ),
        ContextResourceKind::External => (PromptSectionRole::Context, PromptSectionKind::External),
    };
    PromptSection {
        role,
        kind,
        source: entry.resource.descriptor.source.clone(),
        reference: Some(entry.injection.source.clone()),
        content: entry.resource.content.clone(),
    }
}

fn section_rank(kind: &ContextResourceKind) -> u8 {
    match kind {
        ContextResourceKind::ProjectInstruction => 0,
        ContextResourceKind::Skill => 1,
        ContextResourceKind::ProjectDocument => 2,
        ContextResourceKind::External => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextInjection, ContextInjectionLifetime, ContextInjectionRequester, ContextScope,
    };
    use phenix_core::{
        ContextDescriptor, ContextResourceId, ContextResourceRevision, ContextRevisionId,
    };

    fn entry(
        sequence: u64,
        id: &str,
        kind: ContextResourceKind,
        source: &str,
    ) -> ProjectedContextEntry {
        let resource_id = ContextResourceId::parse(id).unwrap();
        let revision = ContextRevisionId::parse(format!("revision-{sequence}-{id}")).unwrap();
        let reference = ExactContextReference {
            resource_id: resource_id.clone(),
            revision: revision.clone(),
        };
        ProjectedContextEntry {
            injection: ContextInjection {
                sequence,
                execution_id: "exec".into(),
                source: reference,
                requester: ContextInjectionRequester::ContextPolicy,
                lifetime: ContextInjectionLifetime::Execution,
                reason: "test".into(),
            },
            resource: ContextResourceRevision {
                descriptor: ContextDescriptor {
                    resource_id,
                    revision,
                    kind,
                    source: source.into(),
                    scope: ContextScope::Workspace,
                    content_identity: format!("content-{sequence}"),
                    estimated_bytes: source.len() as u64,
                },
                content: source.as_bytes().to_vec().into(),
            },
        }
    }

    #[test]
    fn assembly_uses_stable_privilege_order_not_load_order() {
        let projection = ExecutionContextProjection {
            execution_id: "exec".into(),
            entries: vec![
                entry(1, "doc", ContextResourceKind::ProjectDocument, "README.md"),
                entry(
                    3,
                    "skill",
                    ContextResourceKind::Skill,
                    "skills/review/SKILL.md",
                ),
                entry(
                    2,
                    "rules",
                    ContextResourceKind::ProjectInstruction,
                    "AGENTS.md",
                ),
                entry(0, "external", ContextResourceKind::External, "search"),
            ],
        };

        let assembly = assemble_prompt(&projection);
        assert_eq!(
            assembly
                .sections
                .iter()
                .map(|section| section.kind)
                .collect::<Vec<_>>(),
            vec![
                PromptSectionKind::HarnessIdentity,
                PromptSectionKind::ProjectInstruction,
                PromptSectionKind::Skill,
                PromptSectionKind::ProjectDocument,
                PromptSectionKind::External,
            ]
        );
        assert!(assembly.sections[..3]
            .iter()
            .all(|section| section.role == PromptSectionRole::Instruction));
        assert!(assembly.sections[3..]
            .iter()
            .all(|section| section.role == PromptSectionRole::Context));
    }

    #[test]
    fn exact_context_revision_is_included_once() {
        let duplicate = entry(
            1,
            "rules",
            ContextResourceKind::ProjectInstruction,
            "AGENTS.md",
        );
        let mut later = duplicate.clone();
        later.injection.sequence = 2;
        let assembly = assemble_prompt(&ExecutionContextProjection {
            execution_id: "exec".into(),
            entries: vec![later, duplicate],
        });

        assert_eq!(assembly.sections.len(), 2);
        assert_eq!(assembly.sections[1].source, "AGENTS.md");
    }

    #[test]
    fn fixed_identity_describes_default_runtime_surfaces() {
        assert!(PHENIX_HARNESS_IDENTITY.contains("workspace-backed shell tool"));
        assert!(PHENIX_HARNESS_IDENTITY.contains("durable session history"));
        assert!(PHENIX_HARNESS_IDENTITY.contains("persistent memory"));
        assert!(PHENIX_HARNESS_IDENTITY.contains("Active skills"));
    }

    #[test]
    fn fixed_identity_is_present_without_context() {
        let assembly = assemble_prompt(&ExecutionContextProjection {
            execution_id: "exec".into(),
            entries: Vec::new(),
        });

        assert_eq!(assembly.sections.len(), 1);
        let identity = &assembly.sections[0];
        assert_eq!(identity.role, PromptSectionRole::Instruction);
        assert_eq!(identity.kind, PromptSectionKind::HarnessIdentity);
        assert_eq!(identity.source, "phenix");
        assert_eq!(identity.reference, None);
        assert_eq!(
            identity.content,
            PHENIX_HARNESS_IDENTITY.as_bytes().to_vec().into()
        );
    }

    #[test]
    fn exact_reference_breaks_same_class_sequence_ties() {
        let first = entry(1, "a", ContextResourceKind::Skill, "z-source");
        let second = entry(1, "b", ContextResourceKind::Skill, "a-source");

        let left = assemble_prompt(&ExecutionContextProjection {
            execution_id: "exec".into(),
            entries: vec![second.clone(), first.clone()],
        });
        let right = assemble_prompt(&ExecutionContextProjection {
            execution_id: "exec".into(),
            entries: vec![first, second],
        });

        assert_eq!(left, right);
        assert_eq!(
            left.sections
                .iter()
                .skip(1)
                .map(|section| section.source.as_str())
                .collect::<Vec<_>>(),
            vec!["z-source", "a-source"]
        );
    }
}
