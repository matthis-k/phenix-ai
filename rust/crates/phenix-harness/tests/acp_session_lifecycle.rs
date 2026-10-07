use agent_client_protocol::schema::v1::{
    ContentBlock, NewSessionRequest, PromptRequest, ResumeSessionRequest, TextContent,
};
use phenix_application_interface::{
    CreateSession as AppCreateSession, Operation, Prompt as AppPrompt,
    types::{Content, PromptInput, SessionCreateInput, SessionInfo},
};
use phenix_client_acp::{AcpClient, ClientError, StdioConfig};
use phenix_core::{ContractId, ValueCodec};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

const FIRST: &str = "PHENIX_FIXTURE_CHILD_CLOSE_FIRST";
const SECOND: &str = "PHENIX_FIXTURE_CHILD_CLOSE_SECOND";

fn prompt(session_id: String, text: &str) -> PromptRequest {
    PromptRequest::new(
        session_id,
        vec![ContentBlock::Text(TextContent::new(text.to_owned()))],
    )
}

#[tokio::test]
async fn child_session_cleanup_preserves_controller_and_acp_connection() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let state = std::env::temp_dir().join(format!(
        "phenix-acp-session-lifecycle-{}-{nonce}.sqlite",
        std::process::id()
    ));
    let _ = fs::remove_file(&state);

    let client = AcpClient::new(
        StdioConfig::new(env!("CARGO_BIN_EXE_phenix-acp-fixture"))
            .env("PHENIX_STATE_DB", state.to_string_lossy()),
    );

    let working_directory = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let result = client
        .connect_with(|connection| {
            let working_directory = working_directory.clone();
            async move {
                let controller = connection
                    .new_session(NewSessionRequest::new(working_directory))
                    .await?;

                connection
                    .prompt(prompt(controller.session_id.to_string(), FIRST))
                    .await?;

                // This is the critical post-condition missing from the older orchestration
                // tests: after the model created, prompted, and closed an independent child,
                // the same ACP connection and controller session must accept a fresh turn.
                connection
                    .prompt(prompt(controller.session_id.to_string(), SECOND))
                    .await?;

                Ok::<_, ClientError>(())
            }
        })
        .await;

    assert!(
        result.is_ok(),
        "child-session cleanup poisoned the controller or ACP connection: {result:?}"
    );

    let _ = fs::remove_file(state);
}


#[tokio::test]
async fn application_extensions_preserve_controller_after_child_cleanup() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let state = std::env::temp_dir().join(format!(
        "phenix-acp-application-lifecycle-{}-{nonce}.sqlite",
        std::process::id()
    ));
    let _ = fs::remove_file(&state);

    let client = AcpClient::new(
        StdioConfig::new(env!("CARGO_BIN_EXE_phenix-acp-fixture"))
            .env("PHENIX_STATE_DB", state.to_string_lossy()),
    );
    let working_directory = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let result = client
        .connect_with(|connection| {
            let working_directory = working_directory.clone();
            async move {
                let created = connection
                    .invoke_extension(
                        &ContractId::parse(AppCreateSession::ID).unwrap(),
                        SessionCreateInput {
                            working_directory: working_directory.clone(),
                            title: Some("extension controller".into()),
                        }
                        .to_value(),
                    )
                    .await?;
                let controller =
                    SessionInfo::from_value(&created).expect("create extension returns SessionInfo");

                // The Lua binding does this immediately after application-session creation
                // to populate standard ACP config options. Keep it in the regression because
                // this mixed extension/standard path is what Neovim actually exercises.
                connection
                    .resume_session(ResumeSessionRequest::new(
                        controller.session_id.to_string(),
                        working_directory,
                    ))
                    .await?;

                for marker in [FIRST, SECOND] {
                    connection
                        .invoke_extension(
                            &ContractId::parse(AppPrompt::ID).unwrap(),
                            PromptInput {
                                session_id: controller.session_id.clone(),
                                content: vec![Content::Text {
                                    text: marker.to_owned(),
                                }],
                            }
                            .to_value(),
                        )
                        .await?;
                }

                Ok::<_, ClientError>(())
            }
        })
        .await;

    assert!(
        result.is_ok(),
        "application extension lifecycle poisoned the controller: {result:?}"
    );

    let _ = fs::remove_file(state);
}
