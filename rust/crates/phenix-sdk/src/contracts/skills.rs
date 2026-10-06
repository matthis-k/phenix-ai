use phenix_core::{ComponentInterface, InterfaceId, InterfaceSchema};

pub use phenix_core::{
    SKILL_SERVICE, SkillCommand, SkillDefinition, SkillResponse, skill_service,
};

pub struct SkillInterface;

impl ComponentInterface for SkillInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(SKILL_SERVICE).expect("static skill interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<SkillCommand, SkillResponse>()
    }
}
