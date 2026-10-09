//! Portable, data-only kind-template lowering during candidate preparation.
//!
//! This is Stage D's pure lowering kernel, not a registry of Tool, Skill or
//! Agent kinds. Selected contributions and owner authenticity come from
//! Stage B; Core still validates all emitted canonical contracts and grants.

use serde::{Deserialize, Serialize};
use crate::{PhenixValue, Type};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum TemplateField {
    /// Copy a field from the validated source contribution.
    Field(String),
    /// Add a fixed capability-free structural value.
    Literal(PhenixValue),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum TemplateTarget {
    /// Normalize into an already-supported canonical contribution kind.
    Canonical(String),
    /// Continue lowering through another selected kind provider.
    Kind(String),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct KindEmissionTemplate {
    pub target: TemplateTarget,
    pub fields: BTreeMap<String, TemplateField>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct KindDefinition {
    /// Stable, versioned provider-owned kind identity.
    pub kind: String,
    pub provider_owner: String,
    pub schema: Type,
    pub templates: Vec<KindEmissionTemplate>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct KindInput {
    pub contribution: String,
    pub author: String,
    pub kind: String,
    pub value: PhenixValue,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct KindOutput {
    pub contribution: String,
    pub original_author: String,
    pub provider_owner: String,
    pub path: Vec<String>,
    pub canonical_kind: String,
    pub value: PhenixValue,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum KindLoweringError {
    DuplicateKind(String),
    DuplicateContribution(String),
    MissingKind { kind: String, contribution: String },
    InvalidKindValue { kind: String, contribution: String },
    MissingInputField { contribution: String, field: String },
    KindCycle(Vec<String>),
    OutputBoundExceeded { allowed: usize },
}

/// Freeze selected, authenticated kind definitions and lower selected authors.
///
/// Does not execute any callback, acquire authority, select a provider, or
/// mutate a live generation. Recursive kind emissions are bounded by the
/// explicit candidate limit and rejected if their definition graph cycles.
pub fn lower_kinds(
    definitions: &[KindDefinition],
    inputs: &[KindInput],
    max_outputs: NonZeroUsize,
) -> Result<Vec<KindOutput>, KindLoweringError> {
    let mut kinds = BTreeMap::new();
    for definition in definitions {
        if kinds.insert(definition.kind.clone(), definition).is_some() {
            return Err(KindLoweringError::DuplicateKind(definition.kind.clone()));
        }
    }
    let mut seen = BTreeSet::new();
    for input in inputs {
        if !seen.insert(input.contribution.clone()) {
            return Err(KindLoweringError::DuplicateContribution(
                input.contribution.clone(),
            ));
        }
    }
    let mut normalized = inputs.to_vec();
    normalized.sort_by(|a, b| a.contribution.cmp(&b.contribution));
    let mut output = Vec::new();
    for input in &normalized {
        expand(
            input,
            &input.kind,
            &input.value,
            &kinds,
            &mut Vec::new(),
            &mut output,
            max_outputs.get(),
        )?;
    }
    Ok(output)
}

fn expand(
    input: &KindInput,
    kind: &str,
    data: &PhenixValue,
    kinds: &BTreeMap<String, &KindDefinition>,
    path: &mut Vec<String>,
    outputs: &mut Vec<KindOutput>,
    limit: usize,
) -> Result<(), KindLoweringError> {
    if path.iter().any(|item| item == kind) {
        let mut cycle = path.clone();
        cycle.push(kind.to_owned());
        return Err(KindLoweringError::KindCycle(cycle));
    }
    let Some(definition) = kinds.get(kind) else {
        return Err(KindLoweringError::MissingKind {
            kind: kind.into(),
            contribution: input.contribution.clone(),
        });
    };
    if definition.schema.parse(data).is_err() {
        return Err(KindLoweringError::InvalidKindValue {
            kind: kind.into(),
            contribution: input.contribution.clone(),
        });
    }
    path.push(kind.into());
    let source = match data {
        PhenixValue::Map(fields) => Some(fields),
        _ => None,
    };
    for (index, template) in definition.templates.iter().enumerate() {
        let mut fields = BTreeMap::new();
        for (target, value) in &template.fields {
            let projected = match value {
                TemplateField::Literal(value) => value.clone(),
                TemplateField::Field(field) => {
                    source.and_then(|fields| fields.get(field)).cloned()
                        .ok_or_else(|| KindLoweringError::MissingInputField {
                            contribution: input.contribution.clone(),
                            field: field.clone(),
                        })?
                }
            };
            fields.insert(target.clone(), projected);
        }
        let value = PhenixValue::Map(fields);
        match &template.target {
            TemplateTarget::Canonical(canonical_kind) => {
                if outputs.len() >= limit {
                    return Err(KindLoweringError::OutputBoundExceeded { allowed: limit });
                }
                let mut trace = path.clone();
                trace.push(format!("{index}:{canonical_kind}"));
                outputs.push(KindOutput {
                    contribution: input.contribution.clone(),
                    original_author: input.author.clone(),
                    provider_owner: definition.provider_owner.clone(),
                    path: trace,
                    canonical_kind: canonical_kind.clone(),
                    value,
                });
            }
            TemplateTarget::Kind(next) => {
                expand(input, next, &value, kinds, path, outputs, limit)?;
            }
        }
    }
    path.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(name: &str) -> PhenixValue {
        PhenixValue::Map(BTreeMap::from([
            ("name".into(), PhenixValue::String(name.into())),
        ]))
    }

    fn fixture() -> KindDefinition {
        KindDefinition {
            kind: "example.note@1".into(),
            provider_owner: "example.kind-provider".into(),
            schema: Type::Table(BTreeMap::from([
                (crate::Key::parse("name").unwrap(), Type::String)
            ])),
            templates: vec![KindEmissionTemplate {
                target: TemplateTarget::Canonical("example.resource@1".into()),
                fields: BTreeMap::from([("label".into(), TemplateField::Field("name".into()))]),
            }],
        }
    }

    fn source(id: &str, name: &str) -> KindInput {
        KindInput {
            contribution: id.into(),
            author: format!("author.{id}"),
            kind: "example.note@1".into(),
            value: value(name),
        }
    }

    #[test]
    fn two_independent_consumers_lower_to_identical_canonical_bytes_in_any_order() {
        let definitions = [fixture()];
        let inputs = [source("b", "second"), source("a", "first")];
        let first = lower_kinds(&definitions, &inputs, NonZeroUsize::new(8).unwrap()).unwrap();
        let second = lower_kinds(
            &definitions, &[inputs[1].clone(), inputs[0].clone()],
            NonZeroUsize::new(8).unwrap(),
        ).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        assert_eq!(first[0].original_author, "author.a");
        assert_eq!(first[1].canonical_kind, "example.resource@1");
    }

    #[test]
    fn missing_provider_bad_schema_and_duplicate_authorship_fail() {
        assert!(matches!(
            lower_kinds(&[], &[source("a", "x")], NonZeroUsize::new(2).unwrap()),
            Err(KindLoweringError::MissingKind { .. })
        ));
        let mut malformed = source("a", "x");
        malformed.value = PhenixValue::Bool(true);
        assert!(matches!(
            lower_kinds(&[fixture()], &[malformed], NonZeroUsize::new(2).unwrap()),
            Err(KindLoweringError::InvalidKindValue { .. })
        ));
        let duplicated = source("a", "x");
        assert!(matches!(
            lower_kinds(&[fixture()], &[duplicated.clone(), duplicated],
                NonZeroUsize::new(2).unwrap()),
            Err(KindLoweringError::DuplicateContribution(_))
        ));
    }

    #[test]
    fn recursive_templates_and_expansion_limits_are_rejected() {
        let mut definition = fixture();
        definition.templates[0].target = TemplateTarget::Kind("example.note@1".into());
        definition.templates[0].fields = BTreeMap::from([
            ("name".into(), TemplateField::Field("name".into()))
        ]);
        assert!(matches!(
            lower_kinds(&[definition], &[source("a", "x")],
                NonZeroUsize::new(2).unwrap()),
            Err(KindLoweringError::KindCycle(_))
        ));
        assert!(matches!(
            lower_kinds(&[fixture()], &[source("a", "x"), source("b", "y")],
                NonZeroUsize::new(1).unwrap()),
            Err(KindLoweringError::OutputBoundExceeded { .. })
        ));
    }
}
