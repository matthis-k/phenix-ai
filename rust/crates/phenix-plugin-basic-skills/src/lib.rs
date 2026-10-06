use phenix_core::{
    Authority, ComponentId, ComponentInterface, ComponentManifest, InterfaceId, PermissionId,
    PluginContext, PluginInstance, PluginManifest, ResourceNamespace, SKILL_SERVICE, SkillCommand,
    SkillDefinition, SkillId, SkillResponse, TransactionOp,
};
use phenix_sdk::{StaticPluginComponentDispatch, StaticPluginDefinition};

pub const BASIC_SKILLS_PLUGIN: &str = "phenix.basic-skills";
pub const BASIC_SKILLS_COMPONENT: &str = "phenix.basic-skills";
const BASIC_SKILLS_NAMESPACE: &str = "phenix.basic-skills.state";
const INDEX_KEY: &str = "skills/@all";

const BUILTIN_SKILLS: &[(&str, &[u8])] = &[
    ("architect", include_bytes!("../skills/architect/SKILL.md")),
    ("grilling", include_bytes!("../skills/grilling/SKILL.md")),
    ("implement", include_bytes!("../skills/implement/SKILL.md")),
    ("interrogate", include_bytes!("../skills/interrogate/SKILL.md")),
    ("investigate", include_bytes!("../skills/investigate/SKILL.md")),
    ("pickup", include_bytes!("../skills/pickup/SKILL.md")),
    ("plan", include_bytes!("../skills/plan/SKILL.md")),
    ("reflect", include_bytes!("../skills/reflect/SKILL.md")),
    ("rust", include_bytes!("../skills/rust/SKILL.md")),
    ("ship", include_bytes!("../skills/ship/SKILL.md")),
    (
        "to-questionnaire",
        include_bytes!("../skills/to-questionnaire/SKILL.md"),
    ),
    ("verify", include_bytes!("../skills/verify/SKILL.md")),
    ("write", include_bytes!("../skills/write/SKILL.md")),
];

const REQUIRED_BUILTIN_SKILLS: &[&str] = &["write"];

type BasicSkillsContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

pub struct BasicSkillsInterface;

impl ComponentInterface for BasicSkillsInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(SKILL_SERVICE).expect("static skill interface id is valid")
    }
}

struct SkillStore;

#[phenix_sdk::resource(schema = 1)]
impl SkillStore {}

#[phenix_sdk::component]
struct Api;

#[phenix_sdk::component]
impl Api {
    #[phenix(export("phenix.skills@1"), terminal, priority = 10)]
    fn handle(
        &self,
        context: &phenix_sdk::PluginContext<'_, '_, ()>,
        command: SkillCommand,
    ) -> Result<SkillResponse, String> {
        handle(context, command)
    }
}

#[phenix_sdk::plugin(id = "phenix.basic-skills", authority = persistence_authority())]
pub struct Plugin {
    #[phenix(component, id = "phenix.basic-skills")]
    api: Api,

    #[phenix(resource, id = "phenix.basic-skills.state")]
    _state: phenix_sdk::Durable<SkillStore>,
}

#[must_use]
pub fn basic_skills_manifest() -> PluginManifest {
    Plugin::manifest()
}

#[must_use]
pub fn basic_skills_component_manifest() -> ComponentManifest {
    Plugin::component_manifests()
        .into_iter()
        .next()
        .expect("basic skills plugin has one generated component")
}

#[must_use]
pub fn basic_skills_factory() -> Box<dyn PluginInstance> {
    StaticPluginComponentDispatch::into_plugin_instance(Plugin {
        api: Api,
        _state: phenix_sdk::Durable::new(),
    })
}

#[must_use]
pub fn basic_skills_component_id() -> ComponentId {
    basic_skills_component_manifest().id
}

fn handle(
    context: &BasicSkillsContext<'_, '_>,
    command: SkillCommand,
) -> Result<SkillResponse, String> {
    match command {
        SkillCommand::Register { skill } => {
            write_skill(context, &skill)?;
            Ok(SkillResponse::Skill { skill: Some(skill) })
        }
        SkillCommand::Get { id } => Ok(SkillResponse::Skill {
            skill: read_skill(context, &id)?,
        }),
        SkillCommand::List => Ok(SkillResponse::Skills {
            skills: read_ids(context)?
                .into_iter()
                .map(|id| {
                    read_skill(context, &id)?.ok_or_else(|| format!("missing skill: {id}"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        }),
        SkillCommand::Required => Ok(SkillResponse::Skills {
            skills: REQUIRED_BUILTIN_SKILLS
                .iter()
                .map(|id| builtin_skill(id).expect("required builtin skill exists"))
                .collect(),
        }),
    }
}

fn write_skill(
    context: &BasicSkillsContext<'_, '_>,
    skill: &SkillDefinition,
) -> Result<(), String> {
    if builtin_skill(skill.id.as_str()).is_some() {
        return Err(format!(
            "skill id is owned by the basic-skills plugin: {}",
            skill.id
        ));
    }
    let mut ids = read_persisted_ids(context)?;
    if !ids.contains(&skill.id) {
        ids.push(skill.id.clone());
        ids.sort();
    }
    context
        .kernel
        .transact_durable(
            &namespace(),
            &[
                TransactionOp::Put {
                    key: format!("skill/{}", skill.id),
                    value: serde_json::to_vec(skill).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: INDEX_KEY.into(),
                    value: serde_json::to_vec(&ids).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn builtin_skill(id: &str) -> Option<SkillDefinition> {
    BUILTIN_SKILLS
        .iter()
        .find(|(candidate, _)| *candidate == id)
        .map(|(id, content)| SkillDefinition {
            id: SkillId::parse((*id).to_owned()).expect("static builtin skill id is valid"),
            content: content.to_vec().into(),
        })
}

fn read_skill(
    context: &BasicSkillsContext<'_, '_>,
    id: &SkillId,
) -> Result<Option<SkillDefinition>, String> {
    if let Some(skill) = builtin_skill(id.as_str()) {
        return Ok(Some(skill));
    }
    context
        .kernel
        .read_durable(&namespace(), &format!("skill/{id}"))
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_persisted_ids(context: &BasicSkillsContext<'_, '_>) -> Result<Vec<SkillId>, String> {
    context
        .kernel
        .read_durable(&namespace(), INDEX_KEY)
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn read_ids(context: &BasicSkillsContext<'_, '_>) -> Result<Vec<SkillId>, String> {
    let mut ids = read_persisted_ids(context)?;
    ids.extend(
        BUILTIN_SKILLS
            .iter()
            .map(|(id, _)| SkillId::parse((*id).to_owned()).expect("static builtin skill id is valid")),
    );
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn namespace() -> ResourceNamespace {
    ResourceNamespace::parse(BASIC_SKILLS_NAMESPACE).expect("static namespace is valid")
}

fn persistence_authority() -> Authority {
    Authority::new([
        capability("kernel.persistence.schema"),
        capability("kernel.persistence.read"),
        capability("kernel.persistence.write"),
    ])
}

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).expect("static capability is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_provides_static_skills_and_marks_write_required() {
        let ids = BUILTIN_SKILLS
            .iter()
            .map(|(id, _)| *id)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(ids.contains("write"));
        assert!(ids.contains("rust"));
        assert_eq!(REQUIRED_BUILTIN_SKILLS, &["write"]);
        let write = builtin_skill("write").expect("write skill is packaged");
        assert!(String::from_utf8_lossy(write.content.as_ref()).contains("Must always apply"));
    }

    #[test]
    fn generated_authoring_preserves_stable_identity() {
        let manifest = basic_skills_manifest();
        assert_eq!(manifest.id.as_str(), BASIC_SKILLS_PLUGIN);
        assert!(manifest.services.is_empty());
        assert_eq!(manifest.resource_namespaces, vec![namespace()]);

        let component = basic_skills_component_manifest();
        assert_eq!(component.id.as_str(), BASIC_SKILLS_COMPONENT);
        assert_eq!(component.exports.len(), 1);
        assert_eq!(
            component.exports[0].interface,
            BasicSkillsInterface::interface_id()
        );
    }
}
