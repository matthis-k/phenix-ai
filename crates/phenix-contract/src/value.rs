use crate::{Code, Failure, GenerationDigest, OwnerId, SchemaId, ValueId, ValueLocation};
use std::collections::BTreeMap;
pub const MAX_BYTES: usize = 1 << 20;
pub const MAX_DEPTH: usize = 32;
pub const MAX_COUNT: usize = 65_536;
pub const V2_PREFIX: [u8; 4] = [0x50, 0x56, 2, 0];
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Ref(Box<ValueLocation>),
    Bool(bool),
    I64(i64),
    U64(u64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}
fn invalid(message: &str) -> Failure {
    Failure::new(Code::SchemaRejected, message)
}
impl Value {
    pub fn map<const N: usize>(pairs: [(&str, Value); N]) -> Self {
        Self::Map(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Ref(location) => serde_json::json!({
                "phenix_ref_v2": {
                    "generation": location.generation.as_bytes().as_slice(),
                    "owner": location.owner.as_bytes().as_slice(),
                    "cell": location.cell.as_bytes().as_slice(),
                    "schema": location.schema.as_bytes().as_slice(),
                }
            }),
            Self::Bool(x) => (*x).into(),
            Self::I64(x) => (*x).into(),
            Self::U64(x) => (*x).into(),
            Self::String(x) => x.clone().into(),
            Self::Bytes(x) => serde_json::json!({"bytes": x}),
            Self::List(x) => x.iter().map(Self::to_json).collect(),
            Self::Map(x) => x.iter().map(|(k, v)| (k.clone(), v.to_json())).collect(),
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        let mut bytes = V2_PREFIX.to_vec();
        bytes.extend(self.encode_schema()?);
        if bytes.len() > MAX_BYTES {
            return Err(invalid("payload limit"));
        }
        Ok(bytes)
    }
    pub(crate) fn encode_schema(&self) -> Result<Vec<u8>, Failure> {
        fn len(out: &mut Vec<u8>, n: usize) -> Result<(), Failure> {
            out.extend(
                u32::try_from(n)
                    .map_err(|_| invalid("length overflow"))?
                    .to_be_bytes(),
            );
            Ok(())
        }
        fn put(v: &Value, out: &mut Vec<u8>, depth: usize) -> Result<(), Failure> {
            if depth > MAX_DEPTH {
                return Err(invalid("depth limit"));
            }
            match v {
                Value::Null => out.push(0),
                Value::Ref(location) => {
                    out.push(9);
                    out.extend(location.generation.as_bytes());
                    out.extend(location.owner.as_bytes());
                    out.extend(location.cell.as_bytes());
                    out.extend(location.schema.as_bytes());
                }
                Value::Bool(false) => out.push(1),
                Value::Bool(true) => out.push(2),
                Value::I64(x) => {
                    out.push(3);
                    out.extend(x.to_be_bytes());
                }
                Value::U64(x) => {
                    out.push(4);
                    out.extend(x.to_be_bytes());
                }
                Value::String(x) => {
                    out.push(5);
                    len(out, x.len())?;
                    out.extend(x.as_bytes());
                }
                Value::Bytes(x) => {
                    out.push(6);
                    len(out, x.len())?;
                    out.extend(x);
                }
                Value::List(x) => {
                    if x.len() > MAX_COUNT {
                        return Err(invalid("count limit"));
                    }
                    out.push(7);
                    len(out, x.len())?;
                    for v in x {
                        put(v, out, depth + 1)?;
                    }
                }
                Value::Map(x) => {
                    if x.len() > MAX_COUNT {
                        return Err(invalid("count limit"));
                    }
                    out.push(8);
                    len(out, x.len())?;
                    for (k, v) in x {
                        len(out, k.len())?;
                        out.extend(k.as_bytes());
                        put(v, out, depth + 1)?;
                    }
                }
            }
            if out.len() > MAX_BYTES {
                return Err(invalid("payload limit"));
            }
            Ok(())
        }
        let mut out = Vec::new();
        put(self, &mut out, 0)?;
        Ok(out)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.len() > MAX_BYTES || !bytes.starts_with(&V2_PREFIX) {
            return Err(invalid("invalid value prefix or payload limit"));
        }
        Self::decode_schema(&bytes[4..])
    }
    pub(crate) fn decode_schema(bytes: &[u8]) -> Result<Self, Failure> {
        struct Reader<'a> {
            bytes: &'a [u8],
            at: usize,
        }
        impl Reader<'_> {
            fn take(&mut self, n: usize) -> Result<&[u8], Failure> {
                let end = self
                    .at
                    .checked_add(n)
                    .ok_or_else(|| invalid("length overflow"))?;
                let x = self
                    .bytes
                    .get(self.at..end)
                    .ok_or_else(|| invalid("truncated value"))?;
                self.at = end;
                Ok(x)
            }
            fn n(&mut self) -> Result<usize, Failure> {
                Ok(
                    u32::from_be_bytes(self.take(4)?.try_into().map_err(|_| invalid("length"))?)
                        as usize,
                )
            }
            fn text(&mut self) -> Result<String, Failure> {
                let n = self.n()?;
                String::from_utf8(self.take(n)?.to_vec()).map_err(|_| invalid("invalid UTF-8"))
            }
            fn value(&mut self, depth: usize) -> Result<Value, Failure> {
                if depth > MAX_DEPTH {
                    return Err(invalid("depth limit"));
                }
                Ok(match self.take(1)?[0] {
                    0 => Value::Null,
                    1 => Value::Bool(false),
                    2 => Value::Bool(true),
                    3 => Value::I64(i64::from_be_bytes(
                        self.take(8)?.try_into().map_err(|_| invalid("i64"))?,
                    )),
                    4 => Value::U64(u64::from_be_bytes(
                        self.take(8)?.try_into().map_err(|_| invalid("u64"))?,
                    )),
                    5 => Value::String(self.text()?),
                    6 => {
                        let n = self.n()?;
                        Value::Bytes(self.take(n)?.to_vec())
                    }
                    7 => {
                        let n = self.n()?;
                        if n > MAX_COUNT {
                            return Err(invalid("count limit"));
                        }
                        let mut x = Vec::new();
                        for _ in 0..n {
                            x.push(self.value(depth + 1)?);
                        }
                        Value::List(x)
                    }
                    8 => {
                        let n = self.n()?;
                        if n > MAX_COUNT {
                            return Err(invalid("count limit"));
                        }
                        let mut x = BTreeMap::new();
                        let mut previous: Option<String> = None;
                        for _ in 0..n {
                            let k = self.text()?;
                            if previous.as_ref().is_some_and(|p| p >= &k) {
                                return Err(invalid("unordered or duplicate map key"));
                            }
                            previous = Some(k.clone());
                            x.insert(k, self.value(depth + 1)?);
                        }
                        Value::Map(x)
                    }
                    9 => Value::Ref(Box::new(ValueLocation {
                        generation: GenerationDigest::from_bytes(self.take(32)?)?,
                        owner: OwnerId::from_bytes(self.take(32)?)?,
                        cell: ValueId::from_bytes(self.take(16)?)?,
                        schema: SchemaId::from_bytes(self.take(32)?)?,
                    })),
                    _ => return Err(invalid("unknown tag")),
                })
            }
        }
        if bytes.len() > MAX_BYTES {
            return Err(invalid("payload limit"));
        }
        let mut r = Reader { bytes, at: 0 };
        let v = r.value(0)?;
        if r.at != bytes.len() {
            return Err(invalid("trailing bytes"));
        }
        Ok(v)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_vectors_and_rejections() {
        assert_eq!(
            Value::U64(1).encode().unwrap(),
            [0x50, 0x56, 2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 1]
        );
        let value = Value::map([("z", Value::Bytes(vec![0, 255])), ("a", Value::I64(-4))]);
        assert_eq!(Value::decode(&value.encode().unwrap()).unwrap(), value);
        for bad in [
            vec![9],
            vec![0, 0],
            vec![5, 0, 0, 0, 1, 255],
            vec![4, 0],
            vec![7, 0, 1, 0, 1],
            vec![8, 0, 0, 0, 2, 0, 0, 0, 1, b'a', 0, 0, 0, 0, 1, b'a', 0],
        ] {
            let prefixed = [V2_PREFIX.as_slice(), bad.as_slice()].concat();
            assert!(Value::decode(&prefixed).is_err(), "{bad:?}");
        }
        let mut v = Value::Null;
        for _ in 0..34 {
            v = Value::List(vec![v]);
        }
        assert!(v.encode().is_err());
        assert!(Value::decode(&vec![0; MAX_BYTES + 1]).is_err());
    }
}
