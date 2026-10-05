use phenix_domain::PhenixSchema;
use phenix_model_adapter::ModelAdapterError;
use serde_json::{json, Map, Value};

pub(crate) fn json_schema(schema: &PhenixSchema) -> Result<Value, ModelAdapterError> {
    let schema = match schema {
        PhenixSchema::Any => json!({}),
        PhenixSchema::Never => json!({"not": {}}),
        PhenixSchema::Unit => json!({"type": "null"}),
        PhenixSchema::Bool => json!({"type": "boolean"}),
        PhenixSchema::I64 => json!({"type": "integer"}),
        PhenixSchema::U64 => json!({"type": "integer", "minimum": 0}),
        PhenixSchema::F64 => json!({"type": "number"}),
        PhenixSchema::String => json!({"type": "string"}),
        PhenixSchema::Bytes => json!({"type": "string", "contentEncoding": "base64"}),
        PhenixSchema::Option(item) => {
            json!({"anyOf": [json_schema(item)?, {"type": "null"}]})
        }
        PhenixSchema::Array { item, len } => json!({
            "type": "array",
            "items": json_schema(item)?,
            "minItems": len,
            "maxItems": len,
        }),
        PhenixSchema::List(item) => json!({"type": "array", "items": json_schema(item)?}),
        PhenixSchema::Map(item) => {
            json!({"type": "object", "additionalProperties": json_schema(item)?})
        }
        PhenixSchema::Table(fields) => {
            let properties = fields
                .iter()
                .map(|(key, schema)| Ok((key.as_str().to_owned(), json_schema(schema)?)))
                .collect::<Result<Map<String, Value>, ModelAdapterError>>()?;
            let required = fields
                .keys()
                .map(|key| key.as_str().to_owned())
                .collect::<Vec<_>>();
            json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false,
            })
        }
        PhenixSchema::Variant(_) | PhenixSchema::Callable { .. } | PhenixSchema::Object { .. } => {
            return Err(ModelAdapterError::Unsupported(
                "Phenix callable schema cannot be represented as JSON Schema".to_owned(),
            ));
        }
    };
    Ok(schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn table_schema_becomes_external_json_schema() {
        let schema = PhenixSchema::Table(BTreeMap::from([(
            "value".parse().unwrap(),
            PhenixSchema::String,
        )]));

        let json = json_schema(&schema).unwrap();
        assert_eq!(json["type"], "object");
        assert_eq!(json["properties"]["value"]["type"], "string");
        assert_eq!(json["additionalProperties"], false);
    }
}
