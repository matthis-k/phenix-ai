#![forbid(unsafe_code)]

//! Descriptor-driven source generation shared by language bindings.
//!
//! This crate reads only the fixed application descriptor. It never imports
//! runtime plugins, ACP adapters, or language-host integrations.

use phenix_application_interface::{generate, ApplicationDescriptor};
use phenix_core::Type;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenerationError {
    Descriptor(String),
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Descriptor(message) => {
                write!(formatter, "cannot generate binding API: {message}")
            }
        }
    }
}

impl std::error::Error for GenerationError {}

/// Generates a deterministic Lua application module.
///
/// Native Lua ABI integration, async dispatch, and transport ownership remain
/// handwritten in the binding package. The generated module owns stable
/// application identities, schemas, error kinds, and operation wrappers.
pub fn lua(descriptor: &ApplicationDescriptor) -> Result<String, GenerationError> {
    generate::rust(descriptor).map_err(|error| GenerationError::Descriptor(error.to_string()))?;

    let mut source = String::new();
    source.push_str("-- Generated from the fixed Phenix application descriptor. Do not edit.\n");
    source.push_str("local descriptor = {\n");
    line(
        &mut source,
        1,
        &format!("interface_id = {},", lua_string(descriptor.id.as_str())),
    );

    source.push_str("  capabilities = {\n");
    for (id, capability) in &descriptor.capabilities {
        let dependencies = capability
            .dependencies
            .iter()
            .map(|dependency| lua_string(dependency.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        line(
            &mut source,
            2,
            &format!(
                "[{}] = {{ id = {}, name = {}, dependencies = {{ {} }} }},",
                lua_string(id.as_str()),
                lua_string(id.as_str()),
                lua_string(&binding_name(id.as_str())),
                dependencies,
            ),
        );
    }
    source.push_str("  },\n");

    source.push_str("  types = {\n");
    for (id, schema) in &descriptor.types {
        line(
            &mut source,
            2,
            &format!(
                "[{}] = {{ id = {}, schema = {} }},",
                lua_string(id.as_str()),
                lua_string(id.as_str()),
                schema_literal(schema),
            ),
        );
    }
    source.push_str("  },\n");

    source.push_str("  errors = {\n");
    let mut errors = BTreeSet::new();
    for operation in descriptor.operations.values() {
        if let Some(Type::Variant(variants)) = descriptor.types.get(&operation.error) {
            errors.extend(variants.keys().map(|key| error_kind(key.as_str())));
        }
    }
    for error in &errors {
        line(
            &mut source,
            2,
            &format!("[{}] = {},", lua_string(error), lua_string(error)),
        );
    }
    source.push_str("  },\n");

    source.push_str("  error_variants = {\n");
    for operation in descriptor.operations.values() {
        if let Some(Type::Variant(variants)) = descriptor.types.get(&operation.error) {
            for (variant, schema) in variants {
                let kind = error_kind(variant.as_str());
                line(
                    &mut source,
                    2,
                    &format!(
                        "[{}] = {{ kind = {}, variant = {}, schema = {} }},",
                        lua_string(&kind),
                        lua_string(&kind),
                        lua_string(variant.as_str()),
                        schema_literal(schema),
                    ),
                );
            }
            break;
        }
    }
    source.push_str("  },\n");

    source.push_str("  operations = {\n");
    for (id, operation) in &descriptor.operations {
        line(
            &mut source,
            2,
            &format!(
                "[{}] = {{ id = {}, name = {}, capability = {}, input = {}, output = {}, error = {}, extension = {} }},",
                lua_string(id.as_str()),
                lua_string(id.as_str()),
                lua_string(&binding_name(id.as_str())),
                lua_string(operation.capability.as_str()),
                lua_string(operation.input.as_str()),
                lua_string(operation.output.as_str()),
                lua_string(operation.error.as_str()),
                lua_string(&extension_name(id.as_str())),
            ),
        );
    }
    source.push_str("  },\n");

    source.push_str("  events = {\n");
    for (id, event) in &descriptor.events {
        line(
            &mut source,
            2,
            &format!(
                "[{}] = {{ id = {}, name = {}, capability = {}, payload = {}, ordering = {}, extension = {} }},",
                lua_string(id.as_str()),
                lua_string(id.as_str()),
                lua_string(&binding_name(id.as_str())),
                lua_string(event.capability.as_str()),
                lua_string(event.payload.as_str()),
                lua_string(&format!("{:?}", event.ordering).to_lowercase()),
                lua_string(&extension_name(id.as_str())),
            ),
        );
    }
    source.push_str("  },\n");

    source.push_str("  callbacks = {\n");
    for (id, callback) in &descriptor.callbacks {
        line(
            &mut source,
            2,
            &format!(
                "[{}] = {{ id = {}, name = {}, capability = {}, request = {}, response = {}, semantics = {}, extension = {} }},",
                lua_string(id.as_str()),
                lua_string(id.as_str()),
                lua_string(&binding_name(id.as_str())),
                lua_string(callback.capability.as_str()),
                lua_string(callback.request.as_str()),
                lua_string(callback.response.as_str()),
                lua_string(&format!("{:?}", callback.semantics).to_lowercase()),
                lua_string(&extension_name(id.as_str())),
            ),
        );
    }
    source.push_str("  },\n");
    source.push_str("}\n");

    source.push_str("descriptor.capabilities_by_name = {\n");
    for id in descriptor.capabilities.keys() {
        line(
            &mut source,
            1,
            &format!(
                "[{}] = {},",
                lua_string(&binding_name(id.as_str())),
                lua_string(id.as_str()),
            ),
        );
    }
    source.push_str("}\n");

    source.push_str("descriptor.operations_by_extension = {\n");
    for id in descriptor.operations.keys() {
        line(
            &mut source,
            1,
            &format!(
                "[{}] = {},",
                lua_string(&extension_name(id.as_str())),
                lua_string(id.as_str()),
            ),
        );
    }
    source.push_str("}\n");

    source.push_str("descriptor.events_by_extension = {\n");
    for id in descriptor.events.keys() {
        line(
            &mut source,
            1,
            &format!(
                "[{}] = {},",
                lua_string(&extension_name(id.as_str())),
                lua_string(id.as_str()),
            ),
        );
    }
    source.push_str("}\n");

    source.push_str("descriptor.callbacks_by_extension = {\n");
    for id in descriptor.callbacks.keys() {
        line(
            &mut source,
            1,
            &format!(
                "[{}] = {},",
                lua_string(&extension_name(id.as_str())),
                lua_string(id.as_str()),
            ),
        );
    }
    source.push_str("}\n");

    source.push_str("descriptor.conversions = {\n");
    source.push_str("  operations = {\n");
    for (id, operation) in &descriptor.operations {
        line(
            &mut source,
            2,
            &format!(
                "[{}] = {{ request = {}, result = {}, error = {} }},",
                lua_string(id.as_str()),
                lua_string(operation.input.as_str()),
                lua_string(operation.output.as_str()),
                lua_string(operation.error.as_str()),
            ),
        );
    }
    source.push_str("  },\n");
    source.push_str("  events = {\n");
    for (id, event) in &descriptor.events {
        line(
            &mut source,
            2,
            &format!(
                "[{}] = {{ payload = {} }},",
                lua_string(id.as_str()),
                lua_string(event.payload.as_str()),
            ),
        );
    }
    source.push_str("  },\n");
    source.push_str("  callbacks = {\n");
    for (id, callback) in &descriptor.callbacks {
        line(
            &mut source,
            2,
            &format!(
                "[{}] = {{ request = {}, response = {} }},",
                lua_string(id.as_str()),
                lua_string(callback.request.as_str()),
                lua_string(callback.response.as_str()),
            ),
        );
    }
    source.push_str("  },\n");
    source.push_str("}\n");

    source.push_str("descriptor.has_capability = function(capabilities, capability)\n");
    source.push_str("  local id = descriptor.capabilities_by_name[capability] or capability\n");
    source.push_str("  return descriptor.capabilities[id] ~= nil and capabilities[id] == true\n");
    source.push_str("end\n");

    source.push_str("descriptor.operation = function(id)\n");
    source.push_str("  return descriptor.operations[id] or descriptor.operations[descriptor.operations_by_extension[id]]\n");
    source.push_str("end\n");
    source.push_str("descriptor.event = function(id)\n");
    source.push_str(
        "  return descriptor.events[id] or descriptor.events[descriptor.events_by_extension[id]]\n",
    );
    source.push_str("end\n");
    source.push_str("descriptor.callback = function(id)\n");
    source.push_str("  return descriptor.callbacks[id] or descriptor.callbacks[descriptor.callbacks_by_extension[id]]\n");
    source.push_str("end\n");

    source.push_str("descriptor.bind = function(client)\n");
    source.push_str("  return {\n");
    for id in descriptor.operations.keys() {
        line(
            &mut source,
            2,
            &format!(
                "[{}] = function(input) return client:_invoke_application({}, input) end,",
                lua_string(&binding_name(id.as_str())),
                lua_string(id.as_str()),
            ),
        );
    }
    source.push_str("  }\n");
    source.push_str("end\n");
    source.push_str("return descriptor\n");
    Ok(source)
}

fn schema_literal(schema: &Type) -> String {
    match schema {
        Type::Any => "{ kind = \"any\" }".to_owned(),
        Type::Never => "{ kind = \"never\" }".to_owned(),
        Type::Unit => "{ kind = \"unit\" }".to_owned(),
        Type::Bool => "{ kind = \"bool\" }".to_owned(),
        Type::I64 => "{ kind = \"i64\" }".to_owned(),
        Type::U64 => "{ kind = \"u64\" }".to_owned(),
        Type::F64 => "{ kind = \"f64\" }".to_owned(),
        Type::String => "{ kind = \"string\" }".to_owned(),
        Type::Bytes => "{ kind = \"bytes\" }".to_owned(),
        Type::Option(item) => format!("{{ kind = \"option\", item = {} }}", schema_literal(item)),
        Type::Array { item, len } => format!(
            "{{ kind = \"array\", item = {}, len = {len} }}",
            schema_literal(item)
        ),
        Type::List(item) => format!("{{ kind = \"list\", item = {} }}", schema_literal(item)),
        Type::Map(item) => format!("{{ kind = \"map\", item = {} }}", schema_literal(item)),
        Type::Table(fields) => {
            let fields = fields
                .iter()
                .map(|(key, value)| {
                    format!("[{}] = {}", lua_string(key.as_str()), schema_literal(value))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ kind = \"table\", fields = {{ {fields} }} }}")
        }
        Type::Variant(variants) => {
            let variants = variants
                .iter()
                .map(|(key, value)| {
                    format!("[{}] = {}", lua_string(key.as_str()), schema_literal(value))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ kind = \"variant\", variants = {{ {variants} }} }}")
        }
        Type::Callable {
            contract,
            input,
            output,
        } => format!(
            "{{ kind = \"callable\", contract = {}, input = {}, output = {} }}",
            lua_string(contract.as_str()),
            schema_literal(input),
            schema_literal(output),
        ),
        Type::Object { contract } => format!(
            "{{ kind = \"object\", contract = {} }}",
            lua_string(contract.as_str())
        ),
    }
}

fn line(source: &mut String, indent: usize, value: &str) {
    source.push_str(&"  ".repeat(indent));
    source.push_str(value);
    source.push('\n');
}

fn binding_name(id: &str) -> String {
    id.strip_prefix("phenix.application.")
        .unwrap_or(id)
        .split('@')
        .next()
        .unwrap_or(id)
        .replace(['-', '.', '/'], "_")
}

fn error_kind(name: &str) -> String {
    let mut result = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                result.push('_');
            }
            result.extend(character.to_lowercase());
        } else {
            result.push(character);
        }
    }
    result
}

fn extension_name(id: &str) -> String {
    let suffix = id.strip_prefix("phenix.application.").unwrap_or(id);
    format!("_phenix/{suffix}")
}

fn lua_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_application_interface::application_descriptor;

    #[test]
    fn lua_generation_is_deterministic_and_descriptor_owned() {
        let descriptor = application_descriptor();
        let first = lua(&descriptor).expect("fixed descriptor is Lua-generatable");
        let second = lua(&descriptor).expect("fixed descriptor is Lua-generatable");

        assert_eq!(first, second);
        assert!(first.contains("phenix.application@1"));
        assert!(first.contains("phenix.application.skill-list@1"));
        assert!(first.contains("_phenix/skill-list@1"));
        assert!(first.contains("phenix.application.capability.skills@1"));
        assert!(first.contains("phenix.application.error@1"));
        assert!(first.contains("unsupported_capability"));
        assert!(first.contains("schema = { kind ="));
        assert!(first.contains("client:_invoke_application"));
        assert!(first.contains("[\"skill_list\"] = function"));
        assert!(first.contains("name = \"session_update\""));
        assert!(first.contains("_phenix/session-update@1"));
        assert!(first.contains("name = \"capability_call\""));
        assert!(first.contains("_phenix/capability-call@1"));
        assert!(first.contains("descriptor.has_capability"));
        assert!(first.contains("descriptor.operations_by_extension"));
        assert!(first.contains("descriptor.events_by_extension"));
        assert!(first.contains("descriptor.callbacks_by_extension"));
        assert!(first.contains("descriptor.conversions"));
        assert!(first.contains("request = \"phenix.application.type.session-input@1\""));
        assert!(first.contains("error_variants = {"));
    }
}
