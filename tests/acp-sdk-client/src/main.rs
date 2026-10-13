//! Independent stable-v1 ACP client used only by the P1 product check.
//! Phenix's kernel and runtime do not import this SDK.
use agent_client_protocol::schema::{
    v1::{ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, SessionNotification, TextContent},
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

            let session = connection
                .send_request(NewSessionRequest::new(
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
                ))
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
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new("tools:hello"))],
                ))
                .block_task()
                .await?;
            assert_eq!(format!("{:?}", tool_answer.stop_reason), "EndTurn");
            Ok(())
        })
        .await?;

    // One direct reply and a two-tool turn: at least one text chunk
    // from each reply, plus two starts and two completion updates.
    if updates.load(Ordering::Relaxed) < 6 {
        return Err("official ACP client did not receive all tool lifecycle updates".into());
    }
    println!("official-acp-sdk-v1: initialized, ran tools, received typed updates");
    Ok(())
}
