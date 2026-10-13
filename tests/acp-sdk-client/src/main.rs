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
    sync::{Arc, Mutex},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let binary = std::env::args()
        .nth(1)
        .ok_or("usage: phenix-p1-acp-client-check PHENIX_ACP_EXECUTABLE")?;
    let agent = AcpAgent::from_args([binary])?;
    let updates = Arc::new(Mutex::new(Vec::<String>::new()));
    let receiver_updates = Arc::clone(&updates);

    Client
        .builder()
        .name("phenix-p1-official-sdk-check")
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                // Typed decoding has already succeeded at this point. Keep
                // exact notification content and arrival order for replay.
                receiver_updates
                    .lock()
                    .expect("ACP event list poisoned")
                    .push(format!("{notification:?}"));
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

    // Two initial turns produce at least six typed updates. Loading the
    // same durable session must replay the *same ordered notification sequence*.
    // The SDK decodes both halves independently.
    let events = updates.lock().expect("ACP event list poisoned");
    if events.len() < 12 || events.len() % 2 != 0 {
        return Err("official ACP client missed session/load replay updates".into());
    }
    let (original, replay) = events.split_at(events.len() / 2);
    if original != replay {
        return Err("session/load replay differs from the ordered original updates".into());
    }
    println!("official-acp-sdk-v1: exact typed tool and session/load replay");
    Ok(())
}
