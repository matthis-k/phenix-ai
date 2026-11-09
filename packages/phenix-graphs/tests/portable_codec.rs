use phenix_graph::testing::portable::{GoldenVector, check};
use phenix_graph::{ContractValue, Schema};
#[cfg(feature = "derive")]
use phenix_graph::{InputSchema, LocalPortLayout, PortArgs, PortLayout};

#[cfg(feature = "derive")]
#[derive(ContractValue, PartialEq, Debug)]
#[contract(schema = "codec/locals")]
struct Locals {
    bytes: i64,
    cursor: i64,
    header_end: i64,
    header: i64,
    length: i64,
    end: i64,
    field: i64,
    value: i64,
    out: i64,
    r#type: Option<()>,
}

#[cfg(feature = "derive")]
#[test]
fn decoder_member_names_cannot_shadow_its_state() {
    for tag in [0, 1] {
        let value = Locals {
            bytes: 0,
            cursor: 0,
            header_end: 0,
            header: 0,
            length: 0,
            end: 0,
            field: 0,
            value: 0,
            out: 0,
            r#type: (tag == 1).then_some(()),
        };
        let mut bytes = Vec::new();
        for _ in 0..9 {
            bytes.extend_from_slice(&[8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, tag]);
        let malformed: Vec<_> = (0..bytes.len()).map(|n| &bytes[..n]).collect();
        check(
            &Locals::schema(),
            &[GoldenVector {
                name: "locals",
                value,
                bytes: &bytes,
            }],
            &malformed,
        )
        .unwrap();
    }
}

#[cfg(feature = "derive")]
#[derive(PortArgs, PortLayout, LocalPortLayout)]
struct Raw {
    r#type: i64,
}

#[cfg(feature = "derive")]
#[test]
fn raw_identifiers_use_logical_names_in_all_layouts() {
    let slots = Raw::slots().unwrap();
    assert_eq!(slots.len(), 1);
    // The three independent layout generators must agree on the logical path.
    let mut portable = Vec::new();
    Raw::append_slots("", &mut portable).unwrap();
    let mut local = Vec::new();
    Raw::append_local_slots("", &mut local).unwrap();
    assert_eq!(slots[0].name, portable[0].name);
    assert_eq!(slots[0].name, local[0].name);
    assert_eq!(slots[0].name.as_deref(), Some("type"));
    let _ = Raw::SLOT_TYPE;
    assert_eq!(Raw { r#type: 7 }.r#type, 7);
}

#[derive(PartialEq)]
struct Probe<const MODE: u8>(u8);
impl<const M: u8> ContractValue for Probe<M> {
    fn schema() -> Schema {
        let mut schema = u8::schema();
        match M {
            1 => schema.name.push_str("changed"),
            2 => schema.revision += 1,
            3 => schema.structure.push_str("changed"),
            4 => schema.codec.push_str("changed"),
            5 => schema.structure.clear(),
            _ => {}
        }
        schema
    }
    fn wire_schema() -> std::sync::Arc<str> {
        if M == 6 {
            "nominal-only".into()
        } else {
            Self::schema().identity()
        }
    }
    fn encode(&self) -> Vec<u8> {
        vec![if M == 7 { 0 } else { self.0 }]
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        if M == 8 {
            return Ok(Self(0));
        }
        u8::decode(bytes).map(Self)
    }
}

fn probe<const M: u8>(expected: &Schema) -> Result<(), String> {
    check(
        expected,
        &[GoldenVector {
            name: "one",
            value: Probe::<M>(1),
            bytes: &[1],
        }],
        &[&[]],
    )
}

#[test]
fn conformance_checks_complete_descriptors_and_both_codec_directions() {
    let expected = u8::schema();
    probe::<0>(&expected).unwrap();
    assert!(
        probe::<1>(&expected)
            .unwrap_err()
            .contains("descriptor mismatch")
    );
    assert!(
        probe::<2>(&expected)
            .unwrap_err()
            .contains("descriptor mismatch")
    );
    assert!(
        probe::<3>(&expected)
            .unwrap_err()
            .contains("descriptor mismatch")
    );
    assert!(
        probe::<4>(&expected)
            .unwrap_err()
            .contains("descriptor mismatch")
    );
    assert!(
        probe::<5>(&Probe::<5>::schema())
            .unwrap_err()
            .contains("incomplete")
    );
    assert!(probe::<6>(&expected).unwrap_err().contains("wire identity"));
    assert!(
        probe::<7>(&expected)
            .unwrap_err()
            .contains("encoding differs")
    );
    assert!(
        probe::<8>(&expected)
            .unwrap_err()
            .contains("decoded value differs")
    );
    assert!(
        check::<u8>(&expected, &[], &[])
            .unwrap_err()
            .contains("at least one")
    );
    assert!(
        check(
            &expected,
            &[GoldenVector {
                name: "one",
                value: 1u8,
                bytes: &[1]
            }],
            &[&[2]]
        )
        .unwrap_err()
        .contains("malformed vector 0")
    );
    for missing in ["name", "structure", "codec"] {
        let mut incomplete = expected.clone();
        match missing {
            "name" => incomplete.name.clear(),
            "structure" => incomplete.structure.clear(),
            _ => incomplete.codec.clear(),
        }
        assert!(probe::<0>(&incomplete).unwrap_err().contains("incomplete"));
    }
}
