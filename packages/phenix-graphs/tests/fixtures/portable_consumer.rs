use phenix_graph::{ContractValue, Schema};

#[derive(PartialEq)]
pub struct Number {
    pub number: i64,
}

// Independently written decoder and different Rust member names/layout.
#[derive(PartialEq)]
pub struct Record {
    pub flag: bool,
    pub text: Option<String>,
    pub number: i64,
}

impl ContractValue for Record {
    fn schema() -> Schema {
        Schema::record(
            "fixture/record",
            &[
                ("bytes", false, i64::schema()),
                ("cursor", true, String::schema()),
                ("type", false, bool::schema()),
            ],
        )
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = 8u64.to_le_bytes().to_vec();
        out.extend(self.number.to_le_bytes());
        match &self.text {
            None => {
                out.extend(1u64.to_le_bytes());
                out.push(0);
            }
            Some(text) => {
                out.extend((text.len() as u64 + 1).to_le_bytes());
                out.push(1);
                out.extend(text.as_bytes());
            }
        }
        out.extend(1u64.to_le_bytes());
        out.push(u8::from(self.flag));
        out
    }

    fn decode(mut bytes: &[u8]) -> Result<Self, phenix_graph::Failure> {
        fn field<'a>(bytes: &mut &'a [u8], name: &str) -> Result<&'a [u8], phenix_graph::Failure> {
            let header = bytes.get(..8).ok_or_else(|| phenix_graph::CodecError::TruncatedHeader { field: name.into(), offset: 0, remaining: bytes.len() }.failure())?;
            let wire_length = u64::from_le_bytes(header.try_into().unwrap());
            let length = usize::try_from(wire_length)
                .map_err(|cause| phenix_graph::CodecError::LengthOverflow { field: name.into(), length: wire_length, cause }.failure())?;
            let rest = &bytes[8..];
            let value = rest.get(..length).ok_or_else(|| phenix_graph::CodecError::TruncatedPayload { field: name.into(), offset: 8, length, remaining: rest.len() }.failure())?;
            *bytes = &rest[length..];
            Ok(value)
        }
        let total = bytes.len();
        let number = i64::decode(field(&mut bytes, "bytes")?)?;
        let optional = field(&mut bytes, "cursor")?;
        let text = match optional {
            [0] => None,
            [1, rest @ ..] => Some(String::decode(rest)?),
            _ => return Err(phenix_graph::CodecError::OptionTag { field: "cursor".into(), tag: optional.first().copied(), length: optional.len() }.failure()),
        };
        let flag = bool::decode(field(&mut bytes, "type")?)?;
        if !bytes.is_empty() {
            return Err(phenix_graph::CodecError::TrailingBytes { offset: total - bytes.len(), total }.failure());
        }
        Ok(Self { flag, text, number })
    }
}

impl ContractValue for Number {
    fn schema() -> Schema {
        Schema::record(
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
        )
    }

    fn encode(&self) -> Vec<u8> {
        self.number.to_le_bytes().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, phenix_graph::Failure> {
        let bytes: [u8; 8] = bytes
            .try_into()
            .map_err(|cause| phenix_graph::Failure::external("consumer: expected eight bytes", cause))?;
        Ok(Self {
            number: i64::from_le_bytes(bytes),
        })
    }
}
