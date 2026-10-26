//! Bounded parsing of published model-list API formats.
//!
//! A provider plugin chooses the format and maps returned IDs to a known
//! protocol/endpoint. Discovery alone never grants model execution authority.
pub mod cache;
pub mod http;

use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogFormat {
    OpenAi,
    OpenRouter,
    Anthropic,
    ChatGptSubscription,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPage {
    pub models: Vec<CatalogModel>,
    /// Provider-specific continuation ID, not a Phenix revisioned cursor.
    pub next_provider_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    TooLarge,
    InvalidResponse,
    DuplicateModel,
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str, CatalogError> {
    let s = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(CatalogError::InvalidResponse)?;
    if s.is_empty() || s.chars().any(char::is_control) {
        return Err(CatalogError::InvalidResponse);
    }
    Ok(s)
}

/// Parse a single provider-owned model-list page. The caller must select the
/// actual provider's documented format and verify connection authentication.
/// Neither endpoint nor protocol is inferred from the model name.
pub fn parse(format: CatalogFormat, bytes: &[u8]) -> Result<CatalogPage, CatalogError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| CatalogError::InvalidResponse)?;
    let key = if format == CatalogFormat::ChatGptSubscription {
        "models"
    } else {
        "data"
    };
    let data = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or(CatalogError::InvalidResponse)?;
    let mut models = Vec::with_capacity(data.len());
    let mut ids = BTreeSet::new();
    for item in data {
        if format == CatalogFormat::ChatGptSubscription {
            let visibility = field(item, "visibility")?;
            if visibility != "list" {
                continue;
            }
        }
        let id_key = if format == CatalogFormat::ChatGptSubscription {
            "slug"
        } else {
            "id"
        };
        let id = field(item, id_key)?;
        let name_key = match format {
            CatalogFormat::ChatGptSubscription | CatalogFormat::Anthropic => "display_name",
            CatalogFormat::OpenRouter => "name",
            CatalogFormat::OpenAi => "id",
        };
        let label = field(item, name_key)?;
        if !ids.insert(id) {
            return Err(CatalogError::DuplicateModel);
        }
        models.push(CatalogModel {
            id: id.to_owned(),
            label: label.to_owned(),
        });
    }

    let next_provider_cursor = if format == CatalogFormat::Anthropic {
        let has_more = value
            .get("has_more")
            .and_then(Value::as_bool)
            .ok_or(CatalogError::InvalidResponse)?;
        if has_more {
            Some(field(&value, "last_id")?.to_owned())
        } else {
            None
        }
    } else {
        None
    };
    Ok(CatalogPage {
        models,
        next_provider_cursor,
    })
}

/// Strict OpenAI-compatible `GET /models` convention for the API-key
/// connection: published list envelope, bounded canonical model identities,
/// deterministic revision ordering. Selection of executable model families is
/// a separate provider capability policy, not a discovery transport concern.
pub fn parse_standard_openai_model_ids(bytes: &[u8]) -> Result<Vec<String>, CatalogError> {
    let document: Value =
        serde_json::from_slice(bytes).map_err(|_| CatalogError::InvalidResponse)?;
    if document.get("object").and_then(Value::as_str) != Some("list") {
        return Err(CatalogError::InvalidResponse);
    }
    let parsed = parse(CatalogFormat::OpenAi, bytes)?;
    let mut ids = Vec::with_capacity(parsed.models.len());
    for model in parsed.models {
        if model.id.len() > 128
            || !model
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-:/".contains(&b))
        {
            return Err(CatalogError::InvalidResponse);
        }
        ids.push(model.id);
    }
    ids.sort_unstable();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_model_catalog_has_no_implicit_count_or_byte_rejection() {
        let models = (0..4097)
            .map(|i| serde_json::json!({"id": format!("gpt-{i}"), "metadata": "x".repeat(512)}))
            .collect::<Vec<_>>();
        let body = serde_json::to_vec(&serde_json::json!({
            "object": "list", "data": models
        })).unwrap();
        assert!(body.len() > 1024 * 1024);
        let ids = parse_standard_openai_model_ids(&body).unwrap();
        assert_eq!(ids.len(), 4097);
    }

    #[test]
    fn standard_openai_ids_validate_envelope_and_deterministic_order() {
        let ids = parse_standard_openai_model_ids(
            br#"{"object":"list","data":[{"id":"gpt-5"},{"id":"gpt-4.1"}]}"#,
        )
        .unwrap();
        assert_eq!(ids, vec!["gpt-4.1", "gpt-5"]);
        assert_eq!(
            parse_standard_openai_model_ids(br#"{"data":[{"id":"gpt-5"}]}"#),
            Err(CatalogError::InvalidResponse)
        );
        assert_eq!(
            parse_standard_openai_model_ids(br#"{"object":"list","data":[{"id":"bad\\nid"}]}"#),
            Err(CatalogError::InvalidResponse)
        );
        assert_eq!(
            parse_standard_openai_model_ids(br#"{"object":"list","data":[{"id":"x"},{"id":"x"}]}"#),
            Err(CatalogError::DuplicateModel)
        );
    }

    #[test]
    fn openai_list_preserves_ids_and_order() {
        let page = parse(
            CatalogFormat::OpenAi,
            br#"{"data":[{"id":"gpt-a"},{"id":"gpt-b"}]}"#,
        )
        .unwrap();
        assert_eq!(
            page.models,
            vec![
                CatalogModel {
                    id: "gpt-a".into(),
                    label: "gpt-a".into(),
                },
                CatalogModel {
                    id: "gpt-b".into(),
                    label: "gpt-b".into(),
                },
            ]
        );
        assert_eq!(page.next_provider_cursor, None);
    }

    #[test]
    fn openrouter_uses_gateway_display_labels() {
        let page = parse(
            CatalogFormat::OpenRouter,
            br#"{"data":[{"id":"anthropic/model","name":"Claude through gateway","supported_parameters":["tools"]}]}"#,
        )
        .unwrap();
        assert_eq!(page.models[0].id, "anthropic/model");
        assert_eq!(page.models[0].label, "Claude through gateway");
    }

    #[test]
    fn chatgpt_subscription_uses_separate_catalog_and_filters_visibility() {
        let page = parse(
            CatalogFormat::ChatGptSubscription,
            br#"{"models":[{"slug":"gpt-a","display_name":"GPT A","visibility":"list"},{"slug":"hidden","display_name":"Hidden","visibility":"hidden"},{"slug":"gpt-b","display_name":"GPT B","visibility":"list"}]}"#,
        )
        .unwrap();
        assert_eq!(
            page.models
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-a", "gpt-b"]
        );
        assert_eq!(page.models[0].label, "GPT A");
        assert_eq!(
            parse(CatalogFormat::OpenAi, br#"{"models":[]}"#),
            Err(CatalogError::InvalidResponse)
        );
    }

    #[test]
    fn anthropic_pagination_and_capabilities_do_not_infer_protocol() {
        let first = parse(
            CatalogFormat::Anthropic,
            br#"{"data":[{"id":"claude-a","display_name":"Claude A","capabilities":{"thinking":{"supported":true}}}],"has_more":true,"last_id":"claude-a"}"#,
        )
        .unwrap();
        assert_eq!(first.next_provider_cursor.as_deref(), Some("claude-a"));
        let final_page = parse(
            CatalogFormat::Anthropic,
            br#"{"data":[{"id":"claude-b","display_name":"Claude B"}],"has_more":false,"last_id":"claude-b"}"#,
        )
        .unwrap();
        assert_eq!(final_page.next_provider_cursor, None);
    }

    #[test]
    fn malformed_duplicate_and_oversized_catalogs_reject() {
        assert_eq!(
            parse(CatalogFormat::OpenAi, br#"{"data":null}"#),
            Err(CatalogError::InvalidResponse)
        );
        assert_eq!(
            parse(CatalogFormat::OpenAi, br#"{"data":[{"id":""}]}"#),
            Err(CatalogError::InvalidResponse)
        );
        assert_eq!(
            parse(
                CatalogFormat::OpenAi,
                br#"{"data":[{"id":"a"},{"id":"a"}]}"#
            ),
            Err(CatalogError::DuplicateModel)
        );
        assert_eq!(
            parse(CatalogFormat::OpenAi, &vec![b' '; 1024 * 1024 + 1]),
            Err(CatalogError::InvalidResponse)
        );
        assert_eq!(
            parse(CatalogFormat::Anthropic, br#"{"data":[],"has_more":true}"#),
            Err(CatalogError::InvalidResponse)
        );
    }
}
