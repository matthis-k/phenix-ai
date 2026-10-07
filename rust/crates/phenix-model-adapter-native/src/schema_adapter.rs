use phenix_domain::PhenixSchema;
use phenix_model_adapter::ModelAdapterError;
use serde_json::{Map, Value, json};

pub(crate) fn model_tool_json_schema(
    schema: &PhenixSchema,
) -> Result<Value, ModelAdapterError> {
    match schema {
        PhenixSchema::Unit => Ok(json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false,
        })),
        PhenixSchema::Map(_) | PhenixSchema::Table(_) => json_schema(schema),
        _ => Ok(json!({
            "type": "object",
            "properties": {
                "value": json_schema(schema)?,
            },
            "required": ["value"],
            "additionalProperties": false,
        })),
    }
}

pub(crate) fn model_tool_arguments(
    schema: &PhenixSchema,
    value: Value,
) -> Result<Value, ModelAdapterError> {
    let object = value.as_object().ok_or_else(|| {
        ModelAdapterError::Protocol("model tool arguments must be a JSON object".to_owned())
    })?;
    match schema {
        PhenixSchema::Unit => {
            if object.is_empty() {
                Ok(Value::Null)
            } else {
                Err(ModelAdapterError::Protocol(
                    "unit model tool arguments must be an empty object".to_owned(),
                ))
            }
        }
        PhenixSchema::Map(_) | PhenixSchema::Table(_) => Ok(value),
        _ => object.get("value").cloned().ok_or_else(|| {
            ModelAdapterError::Protocol(
                "model tool arguments are missing the required value field".to_owned(),
            )
        }),
    }
}

fn json_schema(schema: &PhenixSchema) -> Result<Value, ModelAdapterError> {
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

        let json = model_tool_json_schema(&schema).unwrap();
        assert_eq!(json["type"], "object");
        assert_eq!(json["properties"]["value"]["type"], "string");
        assert_eq!(json["additionalProperties"], false);
    }

    #[test]
    fn unit_tool_schema_uses_empty_object_and_projects_back_to_null() {
        let schema = model_tool_json_schema(&PhenixSchema::Unit).unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"], json!({}));
        assert_eq!(schema["required"], json!([]));
        assert_eq!(schema["additionalProperties"], false);

        assert_eq!(
            model_tool_arguments(&PhenixSchema::Unit, json!({})).unwrap(),
            Value::Null
        );
        assert!(model_tool_arguments(&PhenixSchema::Unit, json!({"value": null})).is_err());
    }

    #[test]
    fn scalar_tool_schema_uses_value_envelope_and_unwraps_before_dispatch() {
        let schema = model_tool_json_schema(&PhenixSchema::U64).unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["value"]["type"], "integer");
        assert_eq!(schema["required"], json!(["value"]));

        assert_eq!(
            model_tool_arguments(&PhenixSchema::U64, json!({"value": 7})).unwrap(),
            json!(7)
        );
    }

    #[test]
    fn nested_unit_keeps_null_semantics() {
        let schema = PhenixSchema::Table(BTreeMap::from([(
            "done".parse().unwrap(),
            PhenixSchema::Unit,
        )]));
        let json = model_tool_json_schema(&schema).unwrap();
        assert_eq!(json["type"], "object");
        assert_eq!(json["properties"]["done"]["type"], "null");
    }
}
