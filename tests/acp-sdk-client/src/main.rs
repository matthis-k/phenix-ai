//! Independent stable-v1 ACP client used only by the P1 product check.
//! Phenix's kernel and runtime do not import this SDK.
use agent_client_protocol::schema::{
    v1::{
        ContentBlock, InitializeRequest, LoadSessionRequest, NewSessionRequest, PromptRequest,
        ResourceLink, SessionId, SessionNotification, TextContent,
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

            // A failed tool call must yield an ACP Failed update. The Basic
            // agent must continue through its second tool and model turn.
            eprintln!("SDK interop: failing tool prompt");
            let failed_tool_answer = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("tools-fail:hello"))],
                ))
                .block_task()
                .await?;
            assert_eq!(format!("{:?}", failed_tool_answer.stop_reason), "EndTurn");

            // A domain model failure must finish its graph and release
            // ownership. The same ACP session must accept another prompt.
            eprintln!("SDK interop: failed model and same-session recovery");
            let model_failure = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("model-error"))],
                ))
                .block_task()
                .await;
            assert!(
                model_failure.is_err(),
                "model failure was accepted as success"
            );
            let resumed = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "sdk-after-model-error",
                    ))],
                ))
                .block_task()
                .await?;
            assert_eq!(format!("{:?}", resumed.stop_reason), "EndTurn");

            // ACP v1 requires resource links. The frontend must preserve the
            // name and URI in the Basic prompt, without opening the resource.
            eprintln!("SDK interop: resource link prompt");
            let resource_answer = connection
                .send_request(PromptRequest::new(
                    session.session_id.clone(),
                    vec![
                        ContentBlock::Text(TextContent::new("reference")),
                        ContentBlock::ResourceLink(ResourceLink::new(
                            "notes",
                            "file:///tmp/phenix-acp-sdk-unreadable.md",
                        )),
                    ],
                ))
                .block_task()
                .await?;
            assert_eq!(format!("{:?}", resource_answer.stop_reason), "EndTurn");

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
        if !turns.iter().any(|event| {
            event.contains("mock:reference")
                && event.contains("Resource link: notes (file:///tmp/phenix-acp-sdk-unreadable.md)")
        }) {
            return Err("official ACP SDK lost the required v1 resource-link content".into());
        }
        // Both prompts issue two tools. Their terminal states are three
        // successes and one failure; the failed tool is not a failed run.
        let started = turns
            .iter()
            .filter(|event| event.contains("ToolCall("))
            .count();
        let completed = turns
            .iter()
            .filter(|event| event.contains("ToolCallUpdate(") && event.contains("Completed"))
            .count();
        let failed = turns
            .iter()
            .filter(|event| event.contains("ToolCallUpdate(") && event.contains("Failed"))
            .count();
        if (started, completed, failed) != (4, 3, 1) {
            return Err("ACP SDK lost successful or failed tool status updates".into());
        }
        if !turns
            .iter()
            .any(|event| event.contains("mock:sdk-after-model-error"))
        {
            return Err("ACP session did not resume after model provider failure".into());
        }
        if !turns
            .iter()
            .any(|event| event.contains("mock:call-1:error:scripted failure|call-2:5"))
        {
            return Err("Basic agent failed to recover after a tool error".into());
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
    let restarted_agent = AcpAgent::from_args([binary.clone()])?;
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
    // A third SDK connection must explicitly load a persisted session
    // before it may submit new work against that session identity.
    let disconnected_session = persisted_session
        .lock()
        .expect("session identity poisoned")
        .clone()
        .ok_or("missing previously persisted ACP session")?;
    // Two live sessions must also keep independent journal replay cursors.

    eprintln!("SDK interop: two isolated sessions");
    let isolated_agent = AcpAgent::from_args([binary])?;
    let isolated_updates = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let receiver_updates = Arc::clone(&isolated_updates);
    let isolated_ids = Arc::new(Mutex::new(None::<(SessionId, SessionId)>));
    let saved_ids = Arc::clone(&isolated_ids);
    Client
        .builder()
        .name("phenix-p1-official-sdk-session-isolation")
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                receiver_updates
                    .lock()
                    .expect("isolated ACP events poisoned")
                    .push((
                        notification.session_id.to_string(),
                        format!("{notification:?}"),
                    ));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(
            isolated_agent,
            |connection: ConnectionTo<Agent>| async move {
                let init = connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                assert_eq!(init.protocol_version, ProtocolVersion::V1);
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
                let alpha = connection
                    .send_request(NewSessionRequest::new(cwd.clone()))
                    .block_task()
                    .await?;
                let beta = connection
                    .send_request(NewSessionRequest::new(cwd.clone()))
                    .block_task()
                    .await?;
                assert_ne!(alpha.session_id, beta.session_id);
                *saved_ids.lock().expect("isolated ACP IDs poisoned") =
                    Some((alpha.session_id.clone(), beta.session_id.clone()));

                // A failed load must leave existing sessions available.
                // Use a stable ID the host cannot have issued itself.
                let unknown_load = connection
                    .send_request(LoadSessionRequest::new(
                        SessionId::new("phenix-never-issued-session"),
                        cwd.clone(),
                    ))
                    .block_task()
                    .await;
                assert!(unknown_load.is_err(), "unknown ACP session/load succeeded");

                // Even a valid persisted identity must not be admitted on
                // a new connection before an explicit session/load.
                let unowned = connection
                    .send_request(PromptRequest::new(
                        disconnected_session,
                        vec![ContentBlock::Text(TextContent::new("must-be-denied"))],
                    ))
                    .block_task()
                    .await;
                assert!(unowned.is_err(), "unloaded ACP session accepted a prompt");

                for (session, prompt) in [
                    (alpha.session_id.clone(), "sdk-isolated-alpha"),
                    (beta.session_id.clone(), "sdk-isolated-beta"),
                ] {
                    let answer = connection
                        .send_request(PromptRequest::new(
                            session,
                            vec![ContentBlock::Text(TextContent::new(prompt))],
                        ))
                        .block_task()
                        .await?;
                    assert_eq!(format!("{:?}", answer.stop_reason), "EndTurn");
                }
                // Only alpha replays. Beta's delivered sequence must remain
                // unchanged even though both sessions share one ACP connection.
                connection
                    .send_request(LoadSessionRequest::new(alpha.session_id, cwd))
                    .block_task()
                    .await?;
                Ok(())
            },
        )
        .await?;

    let (alpha, beta) = isolated_ids
        .lock()
        .expect("isolated ACP IDs poisoned")
        .clone()
        .ok_or("ACP client did not create both isolated sessions")?;
    let events = isolated_updates
        .lock()
        .expect("isolated ACP events poisoned");
    let alpha_updates: Vec<_> = events
        .iter()
        .filter(|(session, _)| session == &alpha.to_string())
        .map(|(_, event)| event.as_str())
        .collect();
    let beta_updates: Vec<_> = events
        .iter()
        .filter(|(session, _)| session == &beta.to_string())
        .map(|(_, event)| event.as_str())
        .collect();
    if alpha_updates.len() < 2 || alpha_updates.len() % 2 != 0 {
        return Err("isolated alpha session did not replay its journal".into());
    }
    let (delivered, replayed) = alpha_updates.split_at(alpha_updates.len() / 2);
    if delivered != replayed {
        return Err("isolated alpha session replay differs from its own updates".into());
    }
    if !delivered
        .iter()
        .any(|event| event.contains("mock:sdk-isolated-alpha"))
        || beta_updates.is_empty()
        || !beta_updates
            .iter()
            .any(|event| event.contains("mock:sdk-isolated-beta"))
        || alpha_updates
            .iter()
            .any(|event| event.contains("sdk-isolated-beta"))
        || beta_updates
            .iter()
            .any(|event| event.contains("sdk-isolated-alpha"))
    {
        return Err("ACP SDK crossed the session journal or observer boundary".into());
    }
    println!("official-acp-sdk-v1: isolated multi-session replay");
    println!("official-acp-sdk-v1: exact replay after load and process restart");
    Ok(())
}
