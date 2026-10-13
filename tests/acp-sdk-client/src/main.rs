//! Independent stable-v1 ACP client used only by the P1 product check.
//! Phenix's kernel and runtime do not import this SDK.
use agent_client_protocol::schema::{
    v1::{ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, SessionNotification, TextContent},
    ProtocolVersion,
};
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use std::{
    error::Error,
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
            if init.protocol_version != ProtocolVersion::V1 {
                return Err(agent_client_protocol::Error::invalid_params(
                    "agent did not negotiate stable ACP v1",
                ));
            }

            let session = connection
                .send_request(NewSessionRequest::new(std::env::current_dir()?))
                .block_task()
                .await?;
            let answer = connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new("sdk-interop"))],
                ))
                .block_task()
                .await?;
            if format!("{:?}", answer.stop_reason) != "EndTurn" {
                return Err(agent_client_protocol::Error::invalid_params(
                    "Basic agent did not complete the ACP prompt",
                ));
            }
            Ok(())
        })
        .await?;

    if updates.load(Ordering::Relaxed) == 0 {
        return Err("official ACP client received no typed session updates".into());
    }
    println!("official-acp-sdk-v1: initialized, prompted, received updates");
    Ok(())
}
