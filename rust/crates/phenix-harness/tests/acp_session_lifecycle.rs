use agent_client_protocol::schema::v1::{
    ContentBlock, NewSessionRequest, PromptRequest, TextContent,
};
use phenix_client_acp::{AcpClient, ClientError, StdioConfig};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

const FIRST: &str = "PHENIX_FIXTURE_CHILD_CLOSE_FIRST";
const SECOND: &str = "PHENIX_FIXTURE_CHILD_CLOSE_SECOND";

fn prompt(session_id: &str, text: &str) -> PromptRequest {
    PromptRequest::new(session_id, vec![ContentBlock::Text(TextContent::new(text))])
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

    let result = client
        .connect_with(|connection| async move {
            let controller = connection
                .new_session(NewSessionRequest::new("/workspace"))
                .await?;

            connection
                .prompt(prompt(&controller.session_id.to_string(), FIRST))
                .await?;

            // This is the critical post-condition missing from the older orchestration
            // tests: after the model created, prompted, and closed an independent child,
            // the same ACP connection and controller session must accept a fresh turn.
            connection
                .prompt(prompt(&controller.session_id.to_string(), SECOND))
                .await?;

            Ok::<_, ClientError>(())
        })
        .await;

    assert!(
        result.is_ok(),
        "child-session cleanup poisoned the controller or ACP connection: {result:?}"
    );

    let _ = fs::remove_file(state);
}
