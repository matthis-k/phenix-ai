//! ChatGPT-plan OAuth model discovery, not the API-key /v1/models shape.
//! Only a provider with fresh, verified OAuth status and exact account scope
//! may use this *untrusted wire-data* parser to offer executable models.
use serde_json::Value;
use std::collections::BTreeSet;

// Provider catalog size is not a product policy. The optional transport
// resource limit is controlled by the user, never compiled into this parser.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatGptModel {
    pub slug: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogError {
    TooLarge,
    InvalidShape,
    InvalidModel,
    DuplicateModel,
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 128
        && slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-:/".contains(&b))
}

fn valid_display_name(label: &str) -> bool {
    !label.is_empty() && label.len() <= 128 && !label.chars().any(char::is_control)
}

/// Preserve server ordering and show only visibility:list, per SIWC docs.
/// A populated catalog does not by itself prove plan scope or inference quota.
pub fn parse_chatgpt_models(body: &[u8]) -> Result<Vec<ChatGptModel>, CatalogError> {
    let json: Value = serde_json::from_slice(body).map_err(|_| CatalogError::InvalidShape)?;
    let models = json
        .get("models")
        .and_then(Value::as_array)
        .ok_or(CatalogError::InvalidShape)?;
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    for entry in models {
        // The documented picker filters visibility before reading a model's
        // display fields. Hidden and future entries need not carry every
        // field required for a visible, selectable model.
        if entry.get("visibility").and_then(Value::as_str) != Some("list") {
            continue;
        }
        let slug = entry
            .get("slug")
            .and_then(Value::as_str)
            .ok_or(CatalogError::InvalidModel)?;
        let label = entry
            .get("display_name")
            .and_then(Value::as_str)
            .ok_or(CatalogError::InvalidModel)?;
        if !valid_slug(slug) || !valid_display_name(label) {
            return Err(CatalogError::InvalidModel);
        }
        if !ids.insert(slug) {
            return Err(CatalogError::DuplicateModel);
        }
        result.push(ChatGptModel {
            slug: slug.to_owned(),
            display_name: label.to_owned(),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_documented_visible_models_in_original_order() {
        let response = br#"{"models":[
          {"slug":"gpt-6.1-sol","display_name":"GPT 6.1 Sol","visibility":"list"},
          {"slug":"hidden-preview","display_name":"Preview","visibility":"hidden"},
          {"slug":"gpt-5.6","display_name":"GPT 5.6","visibility":"list"}
        ]}"#;
        assert_eq!(
            parse_chatgpt_models(response).unwrap(),
            vec![
                ChatGptModel {
                    slug: "gpt-6.1-sol".into(),
                    display_name: "GPT 6.1 Sol".into(),
                },
                ChatGptModel {
                    slug: "gpt-5.6".into(),
                    display_name: "GPT 5.6".into(),
                },
            ]
        );
    }

    #[test]
    fn hidden_catalog_entries_need_no_picker_fields() {
        let response = br#"{"models":[
          {"visibility":"hide"},
          {"visibility":"hidden","slug":"bad slug","metadata":{"future":true}},
          {"visibility":"list","slug":"gpt-test","display_name":"Test"}
        ]}"#;
        assert_eq!(
            parse_chatgpt_models(response).unwrap(),
            vec![ChatGptModel {
                slug: "gpt-test".into(),
                display_name: "Test".into(),
            }]
        );
    }

    #[test]
    fn large_hidden_catalog_keeps_only_authorized_visible_models() {
        // Reproduces account catalogs with hundreds of hidden records and
        // >256 KiB of model metadata: earlier versions reported TooLarge.
        let mut models = (0..512)
            .map(|i| {
                serde_json::json!({
                    "visibility": "hidden",
                    "slug": format!("internal-{i}"),
                    "metadata": "x".repeat(1024),
                })
            })
            .collect::<Vec<_>>();
        models.insert(
            200,
            serde_json::json!({
                "visibility": "list",
                "slug": "gpt-visible",
                "display_name": "Visible model",
            }),
        );
        let body = serde_json::to_vec(&serde_json::json!({ "models": models })).unwrap();
        assert!(body.len() > 256 * 1024);

        assert_eq!(
            parse_chatgpt_models(&body).unwrap(),
            vec![ChatGptModel {
                slug: "gpt-visible".into(),
                display_name: "Visible model".into(),
            }]
        );
    }

    #[test]
    fn large_raw_and_visible_catalogs_have_no_implicit_count_cap() {
        let catalog = serde_json::json!({
            "models": (0..4097).map(|i| serde_json::json!({
                "visibility": "list",
                "slug": format!("gpt-{i}"),
                "display_name": format!("Model {i}"),
            })).collect::<Vec<_>>(),
        });
        let parsed = parse_chatgpt_models(&serde_json::to_vec(&catalog).unwrap()).unwrap();
        assert_eq!(parsed.len(), 4097);
        assert_eq!(parsed[4096].slug, "gpt-4096");
    }

    #[test]
    fn oauth_schema_is_not_the_api_key_list_response() {
        assert_eq!(
            parse_chatgpt_models(br#"{"object":"list","data":[{"id":"gpt-5"}]}"#),
            Err(CatalogError::InvalidShape)
        );
    }

    #[test]
    fn invalid_and_duplicate_models_fail_closed() {
        for body in [
            br#"{"models":[{"slug":"ok","display_name":"First","visibility":"list"},{"slug":"ok","display_name":"Second","visibility":"list"}]}"#.as_slice(),
            br#"{"models":[{"slug":"","display_name":"Missing","visibility":"list"}]}"#,
            br#"{"models":[{"slug":"bad\nslug","display_name":"Wrong","visibility":"list"}]}"#,
            br#"{"models":[{"slug":"ok","display_name":"","visibility":"list"}]}"#,
            br#"{"models":"not-an-array"}"#,
            br#"not-json"#,
        ] {
            assert!(parse_chatgpt_models(body).is_err(), "{body:?}");
        }
    }
}
