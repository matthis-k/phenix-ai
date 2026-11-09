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

    fn decode(mut bytes: &[u8]) -> Result<Self, String> {
        fn field<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], String> {
            let header = bytes.get(..8).ok_or("missing length")?;
            let length = usize::try_from(u64::from_le_bytes(header.try_into().unwrap()))
                .map_err(|_| "length overflow")?;
            let rest = &bytes[8..];
            let value = rest.get(..length).ok_or("missing payload")?;
            *bytes = &rest[length..];
            Ok(value)
        }
        let number = i64::decode(field(&mut bytes)?)?;
        let text = match field(&mut bytes)? {
            [0] => None,
            [1, rest @ ..] => Some(String::decode(rest)?),
            _ => return Err("bad optional tag".into()),
        };
        let flag = bool::decode(field(&mut bytes)?)?;
        if !bytes.is_empty() {
            return Err("trailing bytes".into());
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

    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let bytes: [u8; 8] = bytes
            .try_into()
            .map_err(|_| "consumer: expected eight bytes")?;
        Ok(Self {
            number: i64::from_le_bytes(bytes),
        })
    }
}
