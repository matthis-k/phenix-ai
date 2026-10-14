//! Independent stable-v1 ACP client used only by the P1 product check.
//! Phenix's kernel and runtime do not import this SDK.
use agent_client_protocol::schema::{
    v1::{
        ContentBlock, InitializeRequest, LoadSessionRequest, NewSessionRequest, PromptRequest,
        SessionId, SessionNotification, TextContent,
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
    let agent = AcpAgent::from_args([binary.clone()])?;
    let persisted_session = Arc::new(Mutex::new(None::<SessionId>));
    let first_session = Arc::clone(&persisted_session);
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
            eprintln!("SDK interop: first initialize");
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            assert_eq!(init.protocol_version, ProtocolVersion::V1);

            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            eprintln!("SDK interop: new session");
            let session = connection
                .send_request(NewSessionRequest::new(cwd.clone()))
                .block_task()
                .await?;
            *first_session.lock().expect("session identity poisoned") =
                Some(session.session_id.clone());
            eprintln!("SDK interop: first prompt");
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
            eprintln!("SDK interop: tool prompt");
            let tool_answer = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("tools:hello"))],
                ))
                .block_task()
                .await?;
            assert_eq!(format!("{:?}", tool_answer.stop_reason), "EndTurn");

            eprintln!("SDK interop: replay same process");
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

    eprintln!("SDK interop: first connection complete");
    // The first host sent two prompts, then loaded the same session.
    // A second process must see identical persisted journal entries.
    let original = {
        let events = updates.lock().expect("ACP event list poisoned");
        if events.len() < 12 || events.len() % 2 != 0 {
            return Err("official ACP client missed in-process session/load replay".into());
        }
        let (turns, replay) = events.split_at(events.len() / 2);
        if turns != replay {
            return Err("in-process session/load replay differs from original updates".into());
        }
        turns.to_vec()
    };
    let session_id = persisted_session
        .lock()
        .expect("session identity poisoned")
        .clone()
        .ok_or("ACP host did not return a session ID")?;
    let restart_events = Arc::clone(&updates);
    eprintln!("SDK interop: start second process");
    let restarted_agent = AcpAgent::from_args([binary])?;
    Client
        .builder()
        .name("phenix-p1-official-sdk-restart")
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                restart_events
                    .lock()
                    .expect("ACP restart event list poisoned")
                    .push(format!("{notification:?}"));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(
            restarted_agent,
            |connection: ConnectionTo<Agent>| async move {
                eprintln!("SDK interop: second initialize");
                let init = connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                assert_eq!(init.protocol_version, ProtocolVersion::V1);
                connection
                    .send_request(LoadSessionRequest::new(
                        session_id,
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
                    ))
                    .block_task()
                    .await?;
                Ok(())
            },
        )
        .await?;

    let events = updates.lock().expect("ACP event list poisoned");
    if events.len() != original.len() * 3 || events[original.len() * 2..] != original {
        return Err("separate ACP process failed to replay the exact durable journal".into());
    }
    println!("official-acp-sdk-v1: exact replay after load and process restart");
    Ok(())
}
