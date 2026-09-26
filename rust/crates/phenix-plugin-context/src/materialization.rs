use crate::{
    projection_state::ContextProjectionState, PromptAssembly, PromptSection, PromptSectionKind,
};
use phenix_core::{ArtifactRevision, Bytes};
use phenix_sdk::{
    ContextInvocationMaterialization, ContextProjectionForm, ContextRetention,
    ExactContextReference, ProjectionRevision,
};
use std::collections::BTreeSet;

pub(crate) fn materialize_invocation(
    assembly: &PromptAssembly,
    state: &ContextProjectionState,
    input: Bytes,
    expected_projection: &ProjectionRevision,
) -> Result<ContextInvocationMaterialization, String> {
    if &state.revision != expected_projection {
        return Err(format!(
            "stale context projection: expected revision {} epoch {}, actual revision {} epoch {}",
            expected_projection.revision,
            expected_projection.cache_epoch,
            state.revision.revision,
            state.revision.cache_epoch
        ));
    }

    let mut output = Vec::new();
    let mut represented = BTreeSet::new();
    for section in &assembly.sections {
        let id = section_id(section);
        let Some(item) = state.admitted.get(&id) else {
            continue;
        };
        represented.insert(id.clone());

        if item.retention == ContextRetention::Compact {
            continue;
        }
        match item.form {
            ContextProjectionForm::Full => append_section(
                &mut output,
                section_label(section.kind),
                &section.source,
                section.content.as_ref(),
            ),
            ContextProjectionForm::Reference => append_reference(
                &mut output,
                item.recovery
                    .as_ref()
                    .or(match &item.source {
                        phenix_sdk::ContextSource::Exact { reference } => Some(reference),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        format!(
                            "context reference has no exact recovery source: {}",
                            item.id
                        )
                    })?,
            ),
            ContextProjectionForm::Omitted => {}
        }
    }

    if let Some(checkpoint) = &state.committed_checkpoint {
        represented.insert(format!("phenix:checkpoint:{}", checkpoint.checkpoint_id));
        append_section(
            &mut output,
            "compact-context",
            &checkpoint.checkpoint_id,
            checkpoint.compact_view.as_ref(),
        );
    }

    for item in state.admitted.values() {
        if represented.contains(&item.id)
            || item.form == ContextProjectionForm::Omitted
            || item.retention == ContextRetention::Compact
        {
            continue;
        }
        return Err(format!(
            "committed context item is not materializable from the current projection: {}",
            item.id
        ));
    }

    let cache_prefix_bytes = u64::try_from(output.len())
        .map_err(|_| "materialized cache prefix length exceeds u64".to_owned())?;
    let mut cache_identity_material = state.revision.cache_epoch.to_be_bytes().to_vec();
    cache_identity_material.extend_from_slice(&output);
    let cache_prefix_identity =
        ArtifactRevision::from_content(&cache_identity_material).to_string();

    append_section(&mut output, "request", "user", input.as_ref());
    Ok(ContextInvocationMaterialization {
        input: output.into(),
        projection: state.revision.clone(),
        cache_prefix_bytes,
        cache_prefix_identity,
    })
}

fn section_id(section: &PromptSection) -> String {
    section.reference.as_ref().map_or_else(
        || "phenix:harness-identity".to_owned(),
        |reference| format!("{}@{}", reference.resource_id, reference.revision),
    )
}

fn section_label(kind: PromptSectionKind) -> &'static str {
    match kind {
        PromptSectionKind::HarnessIdentity => "instruction",
        PromptSectionKind::ProjectInstruction => "project-instruction",
        PromptSectionKind::Skill => "skill",
        PromptSectionKind::ProjectDocument => "project-context",
        PromptSectionKind::External => "external-context",
    }
}

fn append_reference(output: &mut Vec<u8>, reference: &ExactContextReference) {
    let marker = format!("{}@{}", reference.resource_id, reference.revision);
    append_section(output, "context-reference", "exact", marker.as_bytes());
}

fn append_section(output: &mut Vec<u8>, kind: &str, source: &str, content: &[u8]) {
    if !output.is_empty() {
        output.extend_from_slice(b"\n");
    }
    output.extend_from_slice(b"--- phenix ");
    output.extend_from_slice(kind.as_bytes());
    output.extend_from_slice(b" [");
    output.extend_from_slice(source.as_bytes());
    output.extend_from_slice(b"] ---\n");
    output.extend_from_slice(content);
    output.extend_from_slice(b"\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ContextResourceId, ContextRevisionId};
    use phenix_sdk::{
        AdmittedContextItem, CachePlacement, ContextSource, ProjectionCheckpoint,
        ToolCallGroupReference,
    };

    fn revision() -> ProjectionRevision {
        ProjectionRevision {
            revision: 4,
            cache_epoch: 2,
        }
    }

    fn exact(id: &str) -> ExactContextReference {
        ExactContextReference {
            resource_id: ContextResourceId::parse(id).unwrap(),
            revision: ContextRevisionId::parse("revision-1").unwrap(),
        }
    }

    fn section(id: Option<ExactContextReference>, content: &[u8]) -> PromptSection {
        PromptSection {
            role: crate::PromptSectionRole::Context,
            kind: PromptSectionKind::ProjectDocument,
            source: "README.md".into(),
            reference: id,
            content: content.to_vec().into(),
        }
    }

    fn item(reference: ExactContextReference) -> AdmittedContextItem {
        AdmittedContextItem {
            id: format!("{}@{}", reference.resource_id, reference.revision),
            source: ContextSource::Exact {
                reference: reference.clone(),
            },
            content_identity: "sha256:content".into(),
            form: ContextProjectionForm::Full,
            cache: CachePlacement::Epoch,
            retention: ContextRetention::Full,
            estimated_tokens: 8,
            recovery: Some(reference),
        }
    }

    #[test]
    fn full_context_precedes_request() {
        let reference = exact("doc");
        let mut state = ContextProjectionState::new("execution-1");
        state.revision = revision();
        let admitted = item(reference.clone());
        state.admitted.insert(admitted.id.clone(), admitted);
        let assembly = PromptAssembly {
            execution_id: "execution-1".into(),
            sections: vec![section(Some(reference), b"context body")],
        };

        let materialized = materialize_invocation(
            &assembly,
            &state,
            Bytes::from(b"request body".to_vec()),
            &revision(),
        )
        .unwrap();
        let bytes = materialized.input.as_ref();
        let context = bytes
            .windows(b"context body".len())
            .position(|window| window == b"context body")
            .unwrap();
        let request = bytes
            .windows(b"request body".len())
            .position(|window| window == b"request body")
            .unwrap();
        assert!(context < request);
    }

    #[test]
    fn request_suffix_does_not_change_cache_prefix_identity() {
        let reference = exact("doc");
        let mut state = ContextProjectionState::new("execution-1");
        state.revision = revision();
        let admitted = item(reference.clone());
        state.admitted.insert(admitted.id.clone(), admitted);
        let assembly = PromptAssembly {
            execution_id: "execution-1".into(),
            sections: vec![section(Some(reference), b"stable context")],
        };

        let first = materialize_invocation(
            &assembly,
            &state,
            Bytes::from(b"request one".to_vec()),
            &revision(),
        )
        .unwrap();
        let second = materialize_invocation(
            &assembly,
            &state,
            Bytes::from(b"request two".to_vec()),
            &revision(),
        )
        .unwrap();

        assert_eq!(first.cache_prefix_identity, second.cache_prefix_identity);
        assert_eq!(first.cache_prefix_bytes, second.cache_prefix_bytes);
        assert!(first.cache_prefix_bytes > 0);
        assert_ne!(first.input, second.input);
    }

    #[test]
    fn cache_epoch_changes_prefix_identity_even_when_prefix_bytes_match() {
        let reference = exact("doc");
        let mut state = ContextProjectionState::new("execution-1");
        state.revision = revision();
        let admitted = item(reference.clone());
        state.admitted.insert(admitted.id.clone(), admitted);
        let assembly = PromptAssembly {
            execution_id: "execution-1".into(),
            sections: vec![section(Some(reference), b"stable context")],
        };
        let first = materialize_invocation(
            &assembly,
            &state,
            Bytes::from(b"request".to_vec()),
            &revision(),
        )
        .unwrap();

        state.revision.cache_epoch += 1;
        let next = state.revision.clone();
        let second =
            materialize_invocation(&assembly, &state, Bytes::from(b"request".to_vec()), &next)
                .unwrap();

        assert_ne!(first.cache_prefix_identity, second.cache_prefix_identity);
    }

    #[test]
    fn reference_never_reexpands_original_content() {
        let reference = exact("doc");
        let mut state = ContextProjectionState::new("execution-1");
        state.revision = revision();
        let mut admitted = item(reference.clone());
        admitted.form = ContextProjectionForm::Reference;
        admitted.retention = ContextRetention::Reference;
        state.admitted.insert(admitted.id.clone(), admitted);
        let assembly = PromptAssembly {
            execution_id: "execution-1".into(),
            sections: vec![section(Some(reference.clone()), b"secret original body")],
        };

        let materialized = materialize_invocation(
            &assembly,
            &state,
            Bytes::from(b"request".to_vec()),
            &revision(),
        )
        .unwrap();
        let text = String::from_utf8(materialized.input.as_ref().to_vec()).unwrap();
        assert!(!text.contains("secret original body"));
        assert!(text.contains(&format!("{}@{}", reference.resource_id, reference.revision)));
    }

    #[test]
    fn committed_checkpoint_replaces_compacted_source_body() {
        let reference = exact("doc");
        let mut state = ContextProjectionState::new("execution-1");
        state.revision = revision();
        let mut admitted = item(reference.clone());
        admitted.retention = ContextRetention::Compact;
        state.admitted.insert(admitted.id.clone(), admitted);
        let checkpoint_id = "checkpoint-1";
        let checkpoint_item_id = format!("phenix:checkpoint:{checkpoint_id}");
        state.admitted.insert(
            checkpoint_item_id.clone(),
            AdmittedContextItem {
                id: checkpoint_item_id.clone(),
                source: ContextSource::Inline {
                    identity: checkpoint_item_id,
                },
                content_identity: "sha256:summary".into(),
                form: ContextProjectionForm::Full,
                cache: CachePlacement::Epoch,
                retention: ContextRetention::Full,
                estimated_tokens: 4,
                recovery: None,
            },
        );
        state.committed_checkpoint = Some(ProjectionCheckpoint {
            checkpoint_id: checkpoint_id.into(),
            execution_id: "execution-1".into(),
            source_revision: ProjectionRevision {
                revision: 3,
                cache_epoch: 1,
            },
            content_identity: "sha256:summary".into(),
            compact_view: Bytes::from(b"compact summary".to_vec()),
            exact_sources: vec![reference.clone()],
            tool_groups: Vec::<ToolCallGroupReference>::new(),
        });
        let assembly = PromptAssembly {
            execution_id: "execution-1".into(),
            sections: vec![section(Some(reference), b"large original body")],
        };

        let materialized = materialize_invocation(
            &assembly,
            &state,
            Bytes::from(b"request".to_vec()),
            &revision(),
        )
        .unwrap();
        let text = String::from_utf8(materialized.input.as_ref().to_vec()).unwrap();
        assert!(text.contains("compact summary"));
        assert!(!text.contains("large original body"));
    }

    #[test]
    fn stale_projection_is_rejected_before_materialization() {
        let assembly = PromptAssembly {
            execution_id: "execution-1".into(),
            sections: Vec::new(),
        };
        let mut state = ContextProjectionState::new("execution-1");
        state.revision = revision();
        let stale = ProjectionRevision {
            revision: 3,
            cache_epoch: 2,
        };

        assert!(materialize_invocation(
            &assembly,
            &state,
            Bytes::from(b"request".to_vec()),
            &stale,
        )
        .unwrap_err()
        .contains("stale context projection"));
    }
}
