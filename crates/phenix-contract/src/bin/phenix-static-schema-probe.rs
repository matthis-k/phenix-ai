//! A process boundary for independently authored canonical schema vectors.
use phenix_contract::{static_schema::SchemaBytes, StaticField, StaticSchema as S};
const SCHEMAS: [(&str, S); 15] = [
    ("bool", S::Bool),
    ("u64", S::U64),
    ("string", S::String),
    ("list-string", S::List(&S::String)),
    ("option-option-bool", S::Option(&S::Option(&S::Bool))),
    (
        "record",
        S::Struct(&[
            StaticField {
                name: "enabled",
                schema: S::Bool,
            },
            StaticField {
                name: "names",
                schema: S::List(&S::String),
            },
        ]),
    ),
    ("variants", S::Variants(&["Empty", "Ready"])),
    ("ref-string", S::Ref(&S::String)),
    ("ref-bool", S::Ref(&S::Bool)),
    ("i64", S::I64),
    ("bytes", S::Bytes),
    ("empty-record", S::Struct(&[])),
    (
        "nested-ref",
        S::Ref(&S::List(&S::Ref(&S::Option(&S::String)))),
    ),
    (
        "long-record",
        S::Struct(&[
            StaticField {
                name: "a_abcdefghijklmnopqrstuvwxyz",
                schema: S::List(&S::String),
            },
            StaticField {
                name: "z_abcdefghijklmnopqrstuvwxyz",
                schema: S::Option(&S::U64),
            },
        ]),
    ),
    (
        "ref-long-record",
        S::Ref(&S::Struct(&[
            StaticField {
                name: "a_abcdefghijklmnopqrstuvwxyz",
                schema: S::List(&S::String),
            },
            StaticField {
                name: "z_abcdefghijklmnopqrstuvwxyz",
                schema: S::Option(&S::U64),
            },
        ])),
    ),
];
const fn vectors() -> [(&'static str, SchemaBytes, [u8; 32]); 15] {
    let mut out = [("", S::Bool.encode(), [0; 32]); 15];
    let mut i = 0;
    while i < 15 {
        out[i] = (SCHEMAS[i].0, SCHEMAS[i].1.encode(), SCHEMAS[i].1.id_bytes());
        i += 1;
    }
    out
}
static VECTORS: [(&str, SchemaBytes, [u8; 32]); 15] = vectors();
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn main() {
    let vectors:Vec<_>=VECTORS.iter().map(|(name,bytes,id)|serde_json::json!({"name":name,"hex":hex(bytes.bytes()),"id":hex(id)})).collect();
    println!("{}", serde_json::json!(vectors));
}
