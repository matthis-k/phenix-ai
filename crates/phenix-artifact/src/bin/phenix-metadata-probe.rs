//! Independent process boundary for external canonical and rejection vectors.
use phenix_contract::{ContractMetadata, StateRequest, Value, ValueAccessError};
use serde::Deserialize;
use std::io::{self, BufRead};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    kind: String,
    bytes: String,
    #[serde(default)]
    opcode: u32,
}
fn unhex(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if s.len() & 1 != 0 || !s.is_ascii() {
        return Err("hex length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|n| Ok(u8::from_str_radix(&s[n..n + 2], 16)?))
        .collect()
}
fn run(s: &str) -> Result<String, Box<dyn std::error::Error>> {
    let input: Input = serde_json::from_str(s)?;
    let bytes = unhex(&input.bytes)?;
    let output = match input.kind.as_str() {
        "metadata" => ContractMetadata::decode(&bytes)?.encode()?,
        "request" => StateRequest::decode(input.opcode, &bytes)?.encode()?,
        "error" => ValueAccessError::from_value(Value::decode(&bytes)?)?
            .to_value()
            .encode()?,
        _ => return Err("unknown probe kind".into()),
    };
    Ok(output.iter().map(|b| format!("{b:02x}")).collect())
}
fn main() {
    for line in io::stdin().lock().lines() {
        let output = match line
            .map_err(|e| e.to_string())
            .and_then(|line| run(&line).map_err(|e| e.to_string()))
        {
            Ok(bytes) => serde_json::json!({"accepted":true,"bytes":bytes}),
            Err(error) => serde_json::json!({"accepted":false,"error":error}),
        };
        println!("{output}");
    }
}
