#![cfg(feature = "derive")]

use phenix_graph::{ContractValue, InputFields, LocalPortLayout, PortArgs, PortLayout, SlotField};

#[derive(Clone, ContractValue, LocalPortLayout, PortLayout, InputFields)]
#[contract(schema = "input-fields/raw@1")]
struct Raw {
    r#type: i64,
}

#[derive(Clone, LocalPortLayout, PortLayout, InputFields)]
#[allow(dead_code)]
struct Nested {
    #[port(flatten)]
    r#match: Raw,
    #[port(atomic)]
    r#loop: Raw,
}

#[test]
fn both_layouts_share_one_descriptor_owner_and_nested_raw_paths() {
    let _: SlotField<Raw, i64> = Raw::SLOT_TYPE;
    let _: SlotField<Nested, Raw> = Nested::SLOT_MATCH;
    assert_eq!(Nested::SLOT_MATCH.then(Raw::SLOT_TYPE).name(), "match.type");
    assert_eq!(Nested::SLOT_LOOP.name(), "loop");
    // A descriptor describes the field, even when the selected frontier is atomic.
    assert_eq!(Nested::SLOT_LOOP.then(Raw::SLOT_TYPE).name(), "loop.type");
}

#[derive(InputFields)]
struct Generic<'a, T, const N: usize>
where
    T: 'a,
{
    value: &'a T,
    bytes: [u8; N],
}

#[test]
fn descriptors_support_generics_without_layout_or_codec_bounds() {
    struct Domain;
    let _: SlotField<Generic<'_, Domain, 3>, &Domain> = Generic::SLOT_VALUE;
    let _: SlotField<Generic<'_, Domain, 3>, [u8; 3]> = Generic::SLOT_BYTES;
    let value = Generic {
        value: &Domain,
        bytes: [0; 3],
    };
    assert_eq!(value.bytes.len(), 3);
    let _ = value.value;
}

#[derive(PortArgs, InputFields)]
struct Renamed {
    #[slot(name = "public")]
    r#type: i64,
}

#[test]
fn port_args_rename_does_not_rename_the_raw_descriptor() {
    use phenix_graph::InputSchema;
    assert_eq!(Renamed::SLOT_TYPE.name(), "type");
    assert_eq!(Renamed::slots().unwrap()[0].name.as_deref(), Some("public"));
    assert_eq!(Renamed { r#type: 7 }.r#type, 7);
}
