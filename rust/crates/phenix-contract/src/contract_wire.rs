use crate::{Contract, ContractValue, PhenixValue};
use serde::{
    de::Error as _, ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer,
};

impl Serialize for ContractValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut value = serializer.serialize_struct("ContractValue", 2)?;
        value.serialize_field("contract", self.contract())?;
        value.serialize_field("value", self.as_value())?;
        value.end()
    }
}

impl<'de> Deserialize<'de> for ContractValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireContractValue {
            contract: Contract,
            value: PhenixValue,
        }

        let wire = WireContractValue::deserialize(deserializer)?;
        wire.contract.parse(wire.value).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContractId, Key, Type};
    use std::collections::BTreeMap;

    fn key(value: &str) -> Key {
        Key::parse(value).unwrap()
    }

    #[test]
    fn deserialize_reparses_contract_shape() {
        let contract = Contract::new(
            ContractId::parse("fixture.wire@1").unwrap(),
            Type::Table(BTreeMap::from([(key("count"), Type::U64)])),
        );
        let value = contract
            .parse(PhenixValue::Table(BTreeMap::from([(
                key("count"),
                PhenixValue::U64(2),
            )])))
            .unwrap();
        let wire = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            serde_json::from_slice::<ContractValue>(&wire).unwrap(),
            value
        );

        let invalid = serde_json::json!({
            "contract": contract,
            "value": {
                "type": "table",
                "value": {"count": {"type": "string", "value": "wrong"}}
            }
        });
        assert!(serde_json::from_value::<ContractValue>(invalid).is_err());
    }
}
