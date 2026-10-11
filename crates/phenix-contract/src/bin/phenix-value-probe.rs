//! Public-contract test probe. It has no kernel or native-plugin dependency.
use phenix_contract::{
    Code, DynValue, Failure, GenerationDigest, OwnerId, SchemaId, Value, ValueId, ValueLocation,
    ValueSchema, WireProfile,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
};

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Input {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Input>),
    Map(BTreeMap<String, Input>),
    Ref(Location),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    generation: String,
    owner: String,
    cell: String,
    schema: String,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Decode {
        profile: String,
        hex: String,
    },
    Encode {
        profile: String,
        value: Input,
    },
    Schema {
        hex: String,
    },
    Dynamic {
        schema_hex: String,
        profile: String,
        value_hex: String,
    },
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(Code::SchemaRejected, message)
}
fn unhex(text: &str) -> Result<Vec<u8>, Failure> {
    if text.len() & 1 != 0 || !text.is_ascii() {
        return Err(invalid("invalid hex"));
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).map_err(|_| invalid("invalid hex")))
        .collect()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
impl Input {
    fn portable(self) -> Result<Value, Failure> {
        Ok(match self {
            Self::Null => Value::Null,
            Self::Bool(v) => Value::Bool(v),
            Self::I64(v) => Value::I64(v),
            Self::U64(v) => Value::U64(v),
            Self::String(v) => Value::String(v),
            Self::Bytes(v) => Value::Bytes(v),
            Self::List(v) => Value::List(
                v.into_iter()
                    .map(Self::portable)
                    .collect::<Result<_, _>>()?,
            ),
            Self::Map(v) => Value::Map(
                v.into_iter()
                    .map(|(k, v)| Ok((k, v.portable()?)))
                    .collect::<Result<_, Failure>>()?,
            ),
            Self::Ref(v) => Value::Ref(ValueLocation {
                generation: GenerationDigest::from_bytes(&unhex(&v.generation)?)?,
                owner: OwnerId::from_bytes(&unhex(&v.owner)?)?,
                cell: ValueId::from_bytes(&unhex(&v.cell)?)?,
                schema: SchemaId::from_bytes(&unhex(&v.schema)?)?,
            }),
        })
    }
}
fn handle(request: Request) -> Result<serde_json::Value, Failure> {
    match request {
        Request::Decode { profile, hex: text } => {
            let profile = WireProfile::negotiate(&profile)?;
            let value = Value::decode_profile(&unhex(&text)?, profile)?;
            Ok(
                serde_json::json!({"hex": hex(&value.encode_profile(profile)?), "value": value.to_json()}),
            )
        }
        Request::Encode { profile, value } => {
            let value = value.portable()?;
            Ok(
                serde_json::json!({"hex": hex(&value.encode_profile(WireProfile::negotiate(&profile)?)?)}),
            )
        }
        Request::Schema { hex: text } => {
            let schema = ValueSchema::decode(&unhex(&text)?)?;
            Ok(
                serde_json::json!({"hex": hex(&schema.encode()?), "id": hex(schema.id()?.as_bytes())}),
            )
        }
        Request::Dynamic {
            schema_hex,
            profile,
            value_hex,
        } => {
            let schema = ValueSchema::decode(&unhex(&schema_hex)?)?;
            let value =
                Value::decode_profile(&unhex(&value_hex)?, WireProfile::negotiate(&profile)?)?;
            let dynamic = DynValue::new(&schema, value)?;
            Ok(
                serde_json::json!({"id": hex(dynamic.schema().as_bytes()), "value": dynamic.value().to_json()}),
            )
        }
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|e| invalid(e.to_string()))
            .and_then(|line| serde_json::from_str(&line).map_err(|e| invalid(e.to_string())))
            .and_then(handle);
        match result {
            Ok(value) => println!("{}", serde_json::json!({"ok": value})),
            Err(failure) => println!("{}", serde_json::json!({"error": failure})),
        }
    }
}
