use phenix_graph::{ContractValue, Schema};

pub struct Number {
    pub value: i64,
}

impl ContractValue for Number {
    const SCHEMA: &'static str = "provider-number";

    fn schema_descriptor() -> Option<Schema> {
        Some(Schema::record(
            "fixture/number",
            &[(
                "value",
                false,
                Schema {
                    name: "fixture/signed64".into(),
                    revision: 1,
                    structure: "signed-integer:64".into(),
                    codec: "le-i64-v1".into(),
                },
            )],
        ))
    }

    fn encode(&self) -> Vec<u8> {
        self.value.to_le_bytes().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let bytes: [u8; 8] = bytes
            .try_into()
            .map_err(|_| "provider: expected eight bytes")?;
        Ok(Self {
            value: i64::from_le_bytes(bytes),
        })
    }
}
