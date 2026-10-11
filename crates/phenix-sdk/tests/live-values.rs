use phenix_sdk::{
    live::{Portable, Value, ValueRef},
    LivePortable, Value as WireValue,
};
#[derive(LivePortable, Debug, PartialEq)]
struct Record {
    names: Vec<String>,
    enabled: bool,
    optional: Option<Option<u64>>,
}
#[derive(LivePortable, Debug, PartialEq)]
enum Status {
    Ready,
    Empty,
}
#[test]
fn immutable_without_clone_bound() {
    struct NotClone(u64);
    let v: Value<_> = NotClone(7).into();
    let copied = v.clone();
    assert_eq!(copied.0, 7);
    assert!(std::ptr::eq(v.as_ref(), copied.as_ref()));
}
#[test]
fn independent_structural_types() {
    let v = Record {
        names: vec!["sample".into()],
        enabled: true,
        optional: Some(None),
    };
    assert_eq!(Record::from_value(v.to_value()).unwrap(), v);
    assert!(Record::SCHEMA.owned().unwrap().accepts(&v.to_value()));
    let mut malformed = v.to_value();
    if let WireValue::Map(ref mut fields) = malformed {
        fields.insert("extra".into(), WireValue::Bool(true));
    }
    assert!(Record::from_value(malformed).is_err());
    assert_eq!(
        Status::from_value(Status::Ready.to_value()).unwrap(),
        Status::Ready
    );
    assert!(Status::from_value(WireValue::String("missing".into())).is_err());
    assert_eq!(
        ValueRef::<Vec<String>>::SCHEMA
            .owned()
            .unwrap()
            .encode()
            .unwrap(),
        ValueRef::<Vec<String>>::SCHEMA.encode().bytes()
    );
}
