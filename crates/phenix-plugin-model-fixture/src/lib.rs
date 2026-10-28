//! Credential-free, stateless Model contract fixture for the clean rewrite.
//! Only the selected Model operation can execute it. It does not register a provider.
use phenix_basic_contracts::{
    error, session_resource, BasicError, ErrorCode, Message, ModelReply, ModelRequest, Usage,
};

pub fn respond(request: &ModelRequest) -> Result<ModelReply, BasicError> {
    session_resource(&request.session)?;
    if request.run.is_empty() {
        return Err(error(ErrorCode::InvalidInput, "empty run ID"));
    }
    let Some(Message::User { text: prompt }) = request.history.last() else {
        return Err(error(
            ErrorCode::InvalidReply,
            "expected final user message",
        ));
    };
    let text = format!("fixture:{prompt}");
    let output_tokens = (text.len() / 4 + 1) as u64;
    if request
        .limits
        .model_tokens
        .is_some_and(|limit| output_tokens > limit)
    {
        return Err(error(ErrorCode::TurnLimit, "output token budget exceeded"));
    }
    Ok(ModelReply {
        text,
        calls: vec![],
        usage: Usage {
            input_tokens: (prompt.len() / 4 + 1) as u64,
            output_tokens,
        },
    })
}

#[phenix_sdk::plugin(
    id = "phenix.model.fixture",
    version = "1.0.0",
    provide(phenix_basic_contracts::Model, slot = "Model")
)]
mod plugin {
    use super::*;
    use phenix_basic_contracts::Model;
    use phenix_sdk::{CallError, CallResult, Host};

    #[provide(Model::complete)]
    fn complete(host: &Host<'_>, request: ModelRequest) -> CallResult<ModelReply, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        super::respond(&request).map_err(CallError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_basic_contracts::Limits;
    use phenix_sdk::ContractMetadata;

    fn request(text: &str) -> ModelRequest {
        ModelRequest {
            session: "s1".into(),
            run: "r1".into(),
            history: vec![Message::user(text)],
            tools: vec![],
            skills: vec![],
            context: vec![],
            limits: Limits::default(),
        }
    }

    #[test]
    fn plain_reply_and_limit() {
        let mut req = request("hello");
        assert_eq!(respond(&req).unwrap().text, "fixture:hello");
        req.limits.model_tokens = Some(0);
        assert_eq!(respond(&req).unwrap_err().code, ErrorCode::TurnLimit);
    }

    #[test]
    fn exact_output_budget_is_admitted_but_one_less_is_rejected() {
        let mut req = request("boundary");
        let tokens = respond(&req).unwrap().usage.output_tokens;
        req.limits.model_tokens = Some(tokens);
        assert_eq!(respond(&req).unwrap().usage.output_tokens, tokens);
        req.limits.model_tokens = Some(tokens - 1);
        assert_eq!(respond(&req).unwrap_err().code, ErrorCode::TurnLimit);
    }

    #[test]
    fn invalid_caller_input_is_rejected() {
        let mut req = request("hi");
        req.run.clear();
        assert_eq!(respond(&req).unwrap_err().code, ErrorCode::InvalidInput);
        req.run = "r1".into();
        req.history.push(Message::assistant("unsupported", vec![]));
        assert_eq!(respond(&req).unwrap_err().code, ErrorCode::InvalidReply);
        req.session = "bad session".into();
        assert_eq!(respond(&req).unwrap_err().code, ErrorCode::InvalidInput);
    }

    #[test]
    fn metadata_has_one_model_export_and_no_imports() {
        let meta = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert_eq!(meta.provides.len(), 1);
        assert!(meta.requires.is_empty());
    }
}
