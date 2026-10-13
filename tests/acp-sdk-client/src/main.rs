//! Independent stable-v1 ACP client used only by the P1 product check.
//! Phenix's kernel and runtime do not import this SDK.
use agent_client_protocol::schema::{
    v1::{
        ContentBlock, InitializeRequest, LoadSessionRequest, NewSessionRequest, PromptRequest,
        SessionNotification, TextContent,
    },
    ProtocolVersion,
};
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use std::{
    error::Error,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let binary = std::env::args()
        .nth(1)
        .ok_or("usage: phenix-p1-acp-client-check PHENIX_ACP_EXECUTABLE")?;
    let agent = AcpAgent::from_args([binary])?;
    let updates = Arc::new(AtomicUsize::new(0));
    let receiver_updates = Arc::clone(&updates);

    Client
        .builder()
        .name("phenix-p1-official-sdk-check")
        .on_receive_notification(
            async move |_notification: SessionNotification, _connection| {
                receiver_updates.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            assert_eq!(init.protocol_version, ProtocolVersion::V1);

            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let session = connection
                .send_request(NewSessionRequest::new(cwd.clone()))
                .block_task()
                .await?;
            let answer = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("sdk-interop"))],
                ))
                .block_task()
                .await?;
            assert_eq!(format!("{:?}", answer.stop_reason), "EndTurn");

            // Exercise typed tool-call and tool-call-update notifications,
            // not just agent_message_chunk. The SDK decodes every update.
            let tool_answer = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("tools:hello"))],
                ))
                .block_task()
                .await?;
            assert_eq!(format!("{:?}", tool_answer.stop_reason), "EndTurn");

            // Loading the persisted session on the same stdio connection
            // must replay its journal as valid, ordered ACP notifications.
            // This exercises the host's session/load path through the official
            // protocol types, not another handwritten JSON fixture.
            connection
                .send_request(LoadSessionRequest::new(session.session_id, cwd))
                .block_task()
                .await?;
            Ok(())
        })
        .await?;

    // The original turn and the session/load replay each publish at
    // least six typed updates: two message chunks, two tool starts and two
    // tool completions. Missing replay or incompatible wire types fails.
    if updates.load(Ordering::Relaxed) < 12 {
        return Err("official ACP client missed tool updates or session/load replay".into());
    }
    println!("official-acp-sdk-v1: tool updates and loaded-session replay decoded");
    Ok(())
}
