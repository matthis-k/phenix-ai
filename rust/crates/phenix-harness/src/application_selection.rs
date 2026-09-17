use crate::{default_suite_authority, PhenixHarness};
use phenix_application_interface::types::{
    ApplicationError, AuthenticateInput, AuthenticationMethod, AuthenticationMethodKind,
    AuthenticationMethods, AuthenticationResult, ModelInfo, ModelSelectInput, Models, RoutingInfo,
    RoutingProfiles, RoutingSelectInput, SessionInput,
};
use phenix_core::{ModelId, PhenixValue, PluginId, Project, RoutingProfileId, ValueError};
use phenix_plugin_catalog::{
    common_provider_definitions, model_routing_service, options_service, ModelCommand, ModelResponse,
    OptionCommand, OptionContext, OptionKey, OptionResponse, OptionScope, OptionSubjectId,
    OptionValue, RoutingProfile,
};
use phenix_provider_sdk::{
    auth, provider_auth_service, Auth, AuthKind, ProviderAuthCommand, ProviderAuthResponse,
};
use std::collections::BTreeMap;

const MODEL_DEFAULT_OPTION: &str = "model.default";
const DIRECT_PROFILE_PREFIX: &str = "phenix.application.direct.";

pub(crate) fn discover_authentication(
    harness: &mut PhenixHarness,
) -> Result<AuthenticationMethods, ApplicationError> {
    let mut methods = Vec::new();
    for provider in common_provider_definitions() {
        let provider_id = provider.plugin_id().clone();
        for kind in provider.auth_kinds() {
            let (suffix, kind, label) = match kind {
                AuthKind::ApiToken => ("api-key", AuthenticationMethodKind::ApiKey, "API key"),
                AuthKind::OAuth => ("oauth", AuthenticationMethodKind::OAuth, "OAuth"),
            };
            methods.push(AuthenticationMethod {
                id: format!("{}:{suffix}", provider_id),
                name: format!("{} {label}", provider_id),
                description: Some(format!("Authenticate {} using {label}", provider_id)),
                kind,
            });
        }
    }
    Ok(AuthenticationMethods { methods })
}

pub(crate) fn authenticate(
    harness: &mut PhenixHarness,
    input: AuthenticateInput,
) -> Result<AuthenticationResult, ApplicationError> {
    let (provider, method) = input.method_id.rsplit_once(':').ok_or_else(|| {
        ApplicationError::InvalidInput {
            message: "authentication method id must be provider:method".to_owned(),
        }
    })?;
    let provider = PluginId::parse(provider).map_err(|error| ApplicationError::InvalidInput {
        message: format!("invalid authentication provider: {error}"),
    })?;
    match method {
        "api-key" => {
            let secret = input.secret.ok_or_else(|| ApplicationError::InvalidInput {
                message: "API-key authentication requires a secret".to_owned(),
            })?;
            if secret.trim().is_empty() {
                return Err(ApplicationError::InvalidInput {
                    message: "API key must not be empty".to_owned(),
                });
            }
            let token = auth::ApiToken::literal(secret).map_err(|error| {
                ApplicationError::InvalidInput {
                    message: error.to_string(),
                }
            })?;
            invoke_provider_auth(
                harness,
                &provider,
                ProviderAuthCommand::Add {
                    auth: Auth::api_token(token),
                },
            )?;
            set_provider_authenticated(harness, provider, true)?;
            Ok(AuthenticationResult::Authenticated)
        }
        "oauth" => Err(ApplicationError::Failed {
            message: format!(
                "provider {provider} advertises OAuth credentials but does not expose an interactive external authorization flow"
            ),
        }),
        other => Err(ApplicationError::InvalidInput {
            message: format!("unknown authentication method {other:?}"),
        }),
    }
}

pub(crate) fn list_models(
    harness: &mut PhenixHarness,
    input: SessionInput,
) -> Result<Models, ApplicationError> {
    let targets = model_targets(harness)?;
    let available = targets
        .values()
        .map(|target| ModelInfo {
            id: target.model.clone(),
            name: format!("{}/{}", target.provider_plugin, target.model),
            description: Some(format!("Provider {}", target.provider_plugin)),
        })
        .collect();
    let selected_profile = selected_profile(harness, &input.session_id)?;
    let selected = if selected_profile
        .as_ref()
        .is_some_and(|profile| profile.as_str().starts_with(DIRECT_PROFILE_PREFIX))
    {
        selected_profile
            .as_ref()
            .and_then(|profile| profile_by_id(harness, profile).ok().flatten())
            .map(|profile| profile.default_target.model)
    } else {
        None
    };
    Ok(Models { available, selected })
}

pub(crate) fn select_model(
    harness: &mut PhenixHarness,
    input: ModelSelectInput,
) -> Result<Models, ApplicationError> {
    let targets = model_targets(harness)?;
    let target = targets.get(&input.model_id).cloned().ok_or_else(|| {
        ApplicationError::NotFound {
            resource: format!("model {}", input.model_id),
        }
    })?;
    let profile_id = RoutingProfileId::parse(format!(
        "{DIRECT_PROFILE_PREFIX}{}.{}",
        target.provider_plugin, target.model
    ))
    .map_err(|error| ApplicationError::InvalidInput {
        message: format!("cannot create direct model profile id: {error}"),
    })?;
    let profile = RoutingProfile {
        id: profile_id.clone(),
        default_target: target,
        fallback_targets: Vec::new(),
        callable_targets: BTreeMap::new(),
    };
    match profile_by_id(harness, &profile_id)? {
        Some(existing) if existing == profile => {}
        Some(_) => {
            return Err(ApplicationError::Conflict {
                message: format!("direct model profile {profile_id} has incompatible contents"),
            })
        }
        None => {
            let response: ModelResponse = invoke_projected(
                harness,
                &model_routing_service(),
                &ModelCommand::RegisterProfile { profile },
            )?;
            if !matches!(response, ModelResponse::Profile { profile: Some(_) }) {
                return Err(ApplicationError::InvalidResponse {
                    message: "routing service rejected direct model profile".to_owned(),
                });
            }
        }
    }
    set_selected_profile(harness, &input.session_id, &profile_id)?;
    list_models(
        harness,
        SessionInput {
            session_id: input.session_id,
        },
    )
}

pub(crate) fn list_routing_profiles(
    harness: &mut PhenixHarness,
    input: SessionInput,
) -> Result<RoutingProfiles, ApplicationError> {
    let response: ModelResponse = invoke_projected(
        harness,
        &model_routing_service(),
        &ModelCommand::ListProfiles,
    )?;
    let ModelResponse::Profiles { profiles } = response else {
        return Err(ApplicationError::InvalidResponse {
            message: "routing service returned the wrong profile-list response".to_owned(),
        });
    };
    let available: Vec<RoutingInfo> = profiles
        .into_iter()
        .filter(|profile| !profile.id.as_str().starts_with(DIRECT_PROFILE_PREFIX))
        .map(|profile| RoutingInfo {
            name: profile.id.to_string(),
            id: profile.id,
        })
        .collect();
    let selected = selected_profile(harness, &input.session_id)?.filter(|selected| {
        available.iter().any(|profile| profile.id == *selected)
    });
    Ok(RoutingProfiles { available, selected })
}

pub(crate) fn select_routing_profile(
    harness: &mut PhenixHarness,
    input: RoutingSelectInput,
) -> Result<RoutingProfiles, ApplicationError> {
    if input.profile_id.as_str().starts_with(DIRECT_PROFILE_PREFIX) {
        return Err(ApplicationError::InvalidInput {
            message: "direct model profiles are selected through model selection".to_owned(),
        });
    }
    if profile_by_id(harness, &input.profile_id)?.is_none() {
        return Err(ApplicationError::NotFound {
            resource: format!("routing profile {}", input.profile_id),
        });
    }
    set_selected_profile(harness, &input.session_id, &input.profile_id)?;
    list_routing_profiles(
        harness,
        SessionInput {
            session_id: input.session_id,
        },
    )
}

pub(crate) fn selected_profile(
    harness: &mut PhenixHarness,
    session_id: &phenix_core::SessionId,
) -> Result<Option<RoutingProfileId>, ApplicationError> {
    let response: OptionResponse = invoke_projected(
        harness,
        &options_service(),
        &OptionCommand::Resolve {
            key: option_key()?,
            context: OptionContext {
                session: Some(session_subject(session_id)?),
                agent: None,
            },
        },
    )?;
    let OptionResponse::Value { option } = response else {
        return Err(ApplicationError::InvalidResponse {
            message: "options service returned the wrong model.default response".to_owned(),
        });
    };
    let OptionValue::String(value) = option.value else {
        return Err(ApplicationError::InvalidResponse {
            message: "model.default is not a string".to_owned(),
        });
    };
    RoutingProfileId::parse(value)
        .map(Some)
        .map_err(|error| ApplicationError::InvalidResponse {
            message: format!("model.default is not a routing profile id: {error}"),
        })
}

fn model_targets(
    harness: &mut PhenixHarness,
) -> Result<BTreeMap<ModelId, phenix_plugin_catalog::ModelTarget>, ApplicationError> {
    let response: ModelResponse = invoke_projected(
        harness,
        &model_routing_service(),
        &ModelCommand::ListProfiles,
    )?;
    let ModelResponse::Profiles { profiles } = response else {
        return Err(ApplicationError::InvalidResponse {
            message: "routing service returned the wrong profile-list response".to_owned(),
        });
    };
    let mut targets = BTreeMap::new();
    for profile in profiles {
        if profile.id.as_str().starts_with(DIRECT_PROFILE_PREFIX) {
            continue;
        }
        let response: ModelResponse = invoke_projected(
            harness,
            &model_routing_service(),
            &ModelCommand::ListCandidates {
                profile_id: profile.id,
                callable_id: None,
            },
        )?;
        let ModelResponse::Candidates { candidates } = response else {
            return Err(ApplicationError::InvalidResponse {
                message: "routing service returned the wrong candidate response".to_owned(),
            });
        };
        for candidate in candidates {
            let target = candidate.capabilities.target;
            match targets.get(&target.model) {
                None => {
                    targets.insert(target.model.clone(), target);
                }
                Some(existing) if existing == &target => {}
                Some(existing) => {
                    return Err(ApplicationError::Conflict {
                        message: format!(
                            "model id {} resolves to both provider {} and provider {}; application model ids must be unambiguous",
                            target.model, existing.provider_plugin, target.provider_plugin
                        ),
                    })
                }
            }
        }
    }
    Ok(targets)
}

fn profile_by_id(
    harness: &mut PhenixHarness,
    id: &RoutingProfileId,
) -> Result<Option<RoutingProfile>, ApplicationError> {
    let response: ModelResponse = invoke_projected(
        harness,
        &model_routing_service(),
        &ModelCommand::GetProfile { id: id.clone() },
    )?;
    match response {
        ModelResponse::Profile { profile } => Ok(profile),
        _ => Err(ApplicationError::InvalidResponse {
            message: "routing service returned the wrong profile response".to_owned(),
        }),
    }
}

fn set_selected_profile(
    harness: &mut PhenixHarness,
    session_id: &phenix_core::SessionId,
    profile_id: &RoutingProfileId,
) -> Result<(), ApplicationError> {
    let response: OptionResponse = invoke_projected(
        harness,
        &options_service(),
        &OptionCommand::Set {
            key: option_key()?,
            scope: OptionScope::Session(session_subject(session_id)?),
            value: OptionValue::String(profile_id.to_string()),
        },
    )?;
    if matches!(response, OptionResponse::Updated { .. }) {
        Ok(())
    } else {
        Err(ApplicationError::InvalidResponse {
            message: "options service rejected session model selection".to_owned(),
        })
    }
}

fn set_provider_authenticated(
    harness: &mut PhenixHarness,
    provider: PluginId,
    authenticated: bool,
) -> Result<(), ApplicationError> {
    let response: ModelResponse = invoke_projected(
        harness,
        &model_routing_service(),
        &ModelCommand::SetProviderAuthenticated {
            provider_plugin: provider,
            authenticated,
        },
    )?;
    if matches!(response, ModelResponse::Authentication { .. }) {
        Ok(())
    } else {
        Err(ApplicationError::InvalidResponse {
            message: "routing service rejected provider authentication update".to_owned(),
        })
    }
}

fn invoke_provider_auth(
    harness: &mut PhenixHarness,
    provider: &PluginId,
    command: ProviderAuthCommand,
) -> Result<ProviderAuthResponse, ApplicationError> {
    let input = serde_json::to_vec(&command).map_err(|error| ApplicationError::InvalidInput {
        message: error.to_string(),
    })?;
    let output = harness
        .invoke(
            &provider_auth_service(),
            &input,
            &default_suite_authority(),
            Some(provider),
        )
        .map_err(|error| ApplicationError::Failed {
            message: error.to_string(),
        })?;
    serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
        message: error.to_string(),
    })
}

fn invoke_projected<Request, Response>(
    harness: &mut PhenixHarness,
    service: &phenix_core::ServiceId,
    request: &Request,
) -> Result<Response, ApplicationError>
where
    for<'value> PhenixValue: From<&'value Request>,
    for<'value> Response: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    let input = serde_json::to_vec(&PhenixValue::from(request)).map_err(|error| {
        ApplicationError::InvalidInput {
            message: error.to_string(),
        }
    })?;
    let output = harness
        .invoke(service, &input, &default_suite_authority(), None)
        .map_err(|error| ApplicationError::Failed {
            message: error.to_string(),
        })?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    Response::try_from(Project(&output)).map_err(|error| ApplicationError::InvalidResponse {
        message: error.to_string(),
    })
}

fn option_key() -> Result<OptionKey, ApplicationError> {
    OptionKey::parse(MODEL_DEFAULT_OPTION).map_err(|error| ApplicationError::InvalidInput {
        message: error.to_owned(),
    })
}

fn session_subject(
    session_id: &phenix_core::SessionId,
) -> Result<OptionSubjectId, ApplicationError> {
    OptionSubjectId::parse(session_id.as_str()).map_err(|error| ApplicationError::InvalidInput {
        message: error.to_owned(),
    })
}
